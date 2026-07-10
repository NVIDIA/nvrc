// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Consume the cold-plugged `gpu` extension mounted at `/run/kata-extensions/gpu`.
//!
//! With composable images the GPU userspace (libraries, modules, binaries,
//! configs, firmware) lives in the extension rather than the base rootfs, so each
//! helper maps its component to the extension mount. With monolithic images (no
//! extension) they fall back to the canonical rootfs paths.

use std::fs;
use std::path::Path;

use nix::mount::MsFlags;

use crate::execute::foreground;
use crate::macros::ResultExt;

/// The `gpu` extension mount ([`crate::guest_extension_image::MOUNT_BASE`]`/gpu`).
pub const ROOT: &str = "/run/kata-extensions/gpu";

/// Kernel firmware loader's default search path: an empty dir baked into the
/// read-only base image, over which the extension's firmware tree is bound.
const FIRMWARE_DST: &str = "/lib/firmware/nvidia";

/// Loader cache regeneration. `ldconfig`/`LDSO_CACHE` come from the base rootfs;
/// the `_TMP` outputs go to the writable `/run`.
const LDCONFIG: &str = "/sbin/ldconfig";
const LDSO_CACHE: &str = "/etc/ld.so.cache";
const LDSO_CACHE_TMP: &str = "/run/ld.so.cache";
const LDSO_CONF_TMP: &str = "/run/ld.so.conf";

/// Extension GPU libraries, in the multiarch triplet dir mirroring the monolithic
/// base image. nvidia-ctk records where it finds each lib and strips the driver
/// root, so a flat `usr/lib` yields `/usr/lib` container paths its CDI hooks
/// (create-symlinks/update-ldcache) can't reconcile; the triplet dir yields the
/// canonical `/usr/lib/<triplet>` the monolithic path already proves out.
fn lib_dir(root: &str) -> String {
    format!("{root}/usr/lib/{}-linux-gnu", std::env::consts::ARCH)
}

pub fn present() -> bool {
    Path::new(ROOT).is_dir()
}

/// Map a rootfs component path to its location inside the extension, or return
/// it unchanged when the extension is absent.
pub fn path(path: &str) -> String {
    path_in(present(), ROOT, path)
}

fn path_in(present: bool, root: &str, path: &str) -> String {
    if present {
        format!("{root}{path}")
    } else {
        path.to_owned()
    }
}

/// `modprobe --dirname` for `module`: the extension root for NVIDIA modules,
/// `None` otherwise. In-tree modules (`ib_umad`/`mlx5_ib`) stay in the base image.
pub fn modprobe_dirname(module: &str) -> Option<String> {
    modprobe_dirname_in(present(), ROOT, module)
}

fn modprobe_dirname_in(present: bool, root: &str, module: &str) -> Option<String> {
    (present && module.starts_with("nvidia")).then(|| root.to_owned())
}

/// `--driver-root` for `nvidia-ctk cdi generate`. nvidia-ctk strips the driver
/// root from each library's recorded mount path, so passing `<root>` lands the
/// extension libs at the canonical `/usr/lib/<triplet>` in the container. Must be
/// paired with `--dev-root=/` ([`DEV_ROOT`]) so device
/// discovery still finds the real guest `/dev/nvidia*` nodes. `None` for the
/// monolithic image.
pub fn driver_root() -> Option<String> {
    driver_root_in(present(), ROOT)
}

fn driver_root_in(present: bool, root: &str) -> Option<String> {
    present.then(|| root.to_owned())
}

/// `--dev-root` for `nvidia-ctk cdi generate`. Counteracts [`driver_root`]'s
/// strip, which would otherwise drop the real guest `/dev/nvidia*` nodes.
pub const DEV_ROOT: &str = "/";

/// `--nvidia-cdi-hook-path` for `nvidia-ctk cdi generate`. The generated
/// createContainer hooks run `nvidia-cdi-hook` from the guest; it lives in the
/// extension, not nvidia-ctk's `/usr/bin` default, so without this the hooks
/// silently no-op and CUDA breaks. `None` for the monolithic image.
pub fn cdi_hook_path() -> Option<String> {
    cdi_hook_path_in(present(), ROOT)
}

fn cdi_hook_path_in(present: bool, root: &str) -> Option<String> {
    present.then(|| format!("{root}/bin/nvidia-cdi-hook"))
}

/// attestation-agent variant launching the NVIDIA attester; matches
/// `[process.variants.nvidia]` in the extension's `components.toml`.
pub const ATTESTER_VARIANT_NVIDIA: &str = "nvidia";

/// Attester variant kata-agent should use (via `KATA_ATTESTER_VARIANT`). With the
/// GPU extension the GPU must be attested, but the stock attester emits no `gpu0`
/// evidence so a GPU KBS policy can never pass; the `nvidia` variant does. `None`
/// (stock) without the extension: nothing to attest.
pub fn attester_variant() -> Option<&'static str> {
    attester_variant_in(present())
}

fn attester_variant_in(present: bool) -> Option<&'static str> {
    present.then_some(ATTESTER_VARIANT_NVIDIA)
}

pub fn setup() {
    bind_firmware();
    refresh_ldcache();
}

/// Add the extension's lib dir to the loader cache so kata-agent and the CDI
/// hooks it runs resolve the GPU libraries without an inherited `LD_LIBRARY_PATH`.
/// Base rootfs is read-only, so build on `/run` and bind over the cache.
fn refresh_ldcache() {
    if !present() {
        return;
    }
    refresh_ldcache_in(&lib_dir(ROOT));
}

fn refresh_ldcache_in(lib_dir: &str) {
    fs::write(LDSO_CONF_TMP, ldso_conf(lib_dir)).or_panic(format_args!("write {LDSO_CONF_TMP}"));
    // -X: leave the read-only lib dir's symlinks alone; -f/-C: our conf, write to /run.
    foreground(LDCONFIG, &["-X", "-f", LDSO_CONF_TMP, "-C", LDSO_CACHE_TMP]);
    bind_over(LDSO_CACHE_TMP, LDSO_CACHE);
    info!("gpu extension: cached libraries from {lib_dir}");
}

/// Our `-f` conf replaces the base one, so re-include its `.conf.d` before the
/// extension lib dir.
fn ldso_conf(lib_dir: &str) -> String {
    format!("include /etc/ld.so.conf.d/*.conf\n{lib_dir}\n")
}

/// Bind the extension firmware onto the kernel's default search path so
/// `request_firmware` (e.g. GSP) finds it. Skipped when the extension ships none.
fn bind_firmware() {
    bind_firmware_in(&format!("{ROOT}/lib/firmware/nvidia"), FIRMWARE_DST);
}

fn bind_firmware_in(src: &str, dst: &str) {
    if !Path::new(src).is_dir() {
        return;
    }
    bind_over(src, dst);
    info!("gpu extension: bound firmware {src} -> {dst}");
}

/// Bind `src` (file or dir) onto `dst`. `dst` must already exist: the base
/// rootfs is read-only, so the mountpoint is baked in at image build time.
fn bind_over(src: &str, dst: &str) {
    nix::mount::mount(Some(src), dst, None::<&str>, MsFlags::MS_BIND, None::<&str>)
        .or_panic(format_args!("bind {src} -> {dst}"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::require_root;
    use std::fs;
    use tempfile::TempDir;

    // === ROOT / MOUNT_BASE contract ===

    #[test]
    fn test_extension_root_matches_mount_base() {
        assert_eq!(
            ROOT,
            format!("{}/gpu", crate::guest_extension_image::MOUNT_BASE)
        );
    }

    // === path ===

    #[test]
    fn test_path_with_extension() {
        assert_eq!(
            path_in(true, ROOT, "/bin/nvidia-smi"),
            format!("{ROOT}/bin/nvidia-smi")
        );
    }

    #[test]
    fn test_path_without_extension() {
        assert_eq!(path_in(false, ROOT, "/bin/nvidia-smi"), "/bin/nvidia-smi");
    }

    #[test]
    fn test_path_config() {
        assert_eq!(
            path_in(true, ROOT, "/usr/share/nvidia/nvlsm/nvlsm.conf"),
            format!("{ROOT}/usr/share/nvidia/nvlsm/nvlsm.conf")
        );
    }

    // === modprobe_dirname ===

    #[test]
    fn test_modprobe_dirname_nvidia_with_extension() {
        assert_eq!(
            modprobe_dirname_in(true, ROOT, "nvidia"),
            Some(ROOT.to_owned())
        );
        assert_eq!(
            modprobe_dirname_in(true, ROOT, "nvidia-uvm"),
            Some(ROOT.to_owned())
        );
    }

    #[test]
    fn test_modprobe_dirname_nvidia_without_extension() {
        assert_eq!(modprobe_dirname_in(false, ROOT, "nvidia"), None);
    }

    #[test]
    fn test_modprobe_dirname_base_module_with_extension() {
        // In-tree modules ship in the base image, not the extension.
        assert_eq!(modprobe_dirname_in(true, ROOT, "ib_umad"), None);
        assert_eq!(modprobe_dirname_in(true, ROOT, "mlx5_ib"), None);
    }

    // === driver_root ===

    #[test]
    fn test_driver_root_with_extension() {
        assert_eq!(driver_root_in(true, ROOT), Some(ROOT.to_owned()));
    }

    #[test]
    fn test_driver_root_without_extension() {
        assert_eq!(driver_root_in(false, ROOT), None);
    }

    // === cdi_hook_path ===

    #[test]
    fn test_cdi_hook_path_with_extension() {
        assert_eq!(
            cdi_hook_path_in(true, ROOT),
            Some(format!("{ROOT}/bin/nvidia-cdi-hook"))
        );
    }

    #[test]
    fn test_cdi_hook_path_without_extension() {
        assert_eq!(cdi_hook_path_in(false, ROOT), None);
    }

    // === attester_variant ===

    #[test]
    fn test_attester_variant_with_extension() {
        assert_eq!(attester_variant_in(true), Some(ATTESTER_VARIANT_NVIDIA));
    }

    #[test]
    fn test_attester_variant_without_extension() {
        assert_eq!(attester_variant_in(false), None);
    }

    // === public wrappers (monolithic image: no extension mounted) ===

    // The public helpers read the real filesystem via present(). In CI/test
    // environments the extension is never mounted, so they must take the
    // monolithic fallback. Guarded so a host that happens to have the mount does
    // not fail the suite.
    #[test]
    fn test_public_helpers_fall_back_without_extension() {
        if present() {
            return;
        }
        assert_eq!(path("/bin/nvidia-smi"), "/bin/nvidia-smi");
        assert_eq!(modprobe_dirname("nvidia"), None);
        assert_eq!(driver_root(), None);
        assert_eq!(cdi_hook_path(), None);
        assert_eq!(attester_variant(), None);
        setup(); // no-op without the extension
    }

    // === bind_firmware ===

    #[test]
    fn test_bind_firmware_skips_without_src() {
        // No firmware tree in the extension: no-op, and in particular no attempt
        // to bind onto the (nonexistent) destination. Needs no root.
        bind_firmware_in("/nonexistent/firmware/src", "/nonexistent/firmware/dst");
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "root-gated: require_root re-execs the test binary via sudo, which miri cannot emulate"
    )]
    fn test_bind_firmware_binds_when_src_present() {
        require_root();
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();
        fs::write(src.path().join("gsp.bin.txt"), "firmware").unwrap();

        bind_firmware_in(src.path().to_str().unwrap(), dst.path().to_str().unwrap());

        let visible = dst.path().join("gsp.bin.txt");
        assert!(visible.exists());
        assert_eq!(fs::read_to_string(visible).unwrap(), "firmware");

        nix::mount::umount(dst.path()).unwrap();
    }

    // === ldso_conf ===

    #[test]
    fn test_ldso_conf_includes_base_and_extension() {
        assert_eq!(
            ldso_conf(&format!("{ROOT}/usr/lib")),
            format!("include /etc/ld.so.conf.d/*.conf\n{ROOT}/usr/lib\n")
        );
    }

    // === bind_over (needs root for mount(2)) ===

    #[test]
    #[cfg_attr(
        miri,
        ignore = "root-gated: require_root re-execs the test binary via sudo, which miri cannot emulate"
    )]
    fn test_bind_over_makes_source_visible() {
        require_root();
        let src = TempDir::new().unwrap();
        let dst = TempDir::new().unwrap();
        fs::write(src.path().join("gsp.bin.txt"), "firmware").unwrap();

        let src_str = src.path().to_str().unwrap();
        let dst_str = dst.path().to_str().unwrap();
        bind_over(src_str, dst_str);

        let visible = dst.path().join("gsp.bin.txt");
        assert!(visible.exists());
        assert_eq!(fs::read_to_string(visible).unwrap(), "firmware");

        nix::mount::umount(dst.path()).unwrap();
    }
}
