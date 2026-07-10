// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Mount cold-plugged composable VM image extensions before kata-agent starts.
//!
//! For each virtio-blk extension (serial `extension-<name>`) NVRC dm-verity-opens
//! the device with `kata.extension.<name>.verity_params` from the (measured)
//! kernel command line and mounts the EROFS read-only at
//! `/run/kata-extensions/<name>/`.
//!
//! Being confidential-only, NVRC fails closed rather than downgrade: discovery is
//! device-driven and reconciled 1:1 with the command line, with no unmeasured
//! raw-mount fallback. Every extension must be verity-measured.
//!
//!   device + params   -> verify and mount
//!   device, no params -> panic (verity stripped)
//!   params, no device -> panic (mismatch / missing extension)
//!
//! See ARCHITECTURE.md ("Composable Image Extensions") for the design rationale.

use log::info;
use nix::mount::MsFlags;
use std::fs;

use crate::execute::foreground;
use crate::macros::ResultExt;

const CMDLINE: &str = "/proc/cmdline";
const SYS_BLOCK: &str = "/sys/block";
/// Extension mount tree (`<MOUNT_BASE>/<name>`); source of truth for
/// [`crate::gpu_extension::ROOT`].
pub(crate) const MOUNT_BASE: &str = "/run/kata-extensions";
const VERITYSETUP: &str = "/usr/sbin/veritysetup";

/// Prefix for the virtio-blk serial and the dm-verity device-mapper target.
const EXTENSION_PREFIX: &str = "extension-";

/// Kernel cmdline key wrapping a per-extension verity param list:
/// `kata.extension.<name>.verity_params`.
const CMDLINE_KEY_PREFIX: &str = "kata.extension.";
const CMDLINE_KEY_SUFFIX: &str = ".verity_params";

/// dm-verity parameters from `kata.extension.<name>.verity_params`, matching the
/// comma-separated list emitted by the Kata image builder. Hash is sha256.
struct VerityParams {
    root_hash: String,
    salt: String,
    data_blocks: u64,
    data_block_size: u64,
    hash_block_size: u64,
}

/// Mount every cold-plugged extension; no-op on non-composable images.
pub fn mount_all() {
    let cmdline = fs::read_to_string(CMDLINE).or_panic(format_args!("read {CMDLINE}"));
    let params = parse_extensions(&cmdline);
    let devices = discover_extensions(SYS_BLOCK);
    for (name, dev, verity) in plan_mounts(&params, &devices) {
        mount_extension(name, dev, verity);
    }
}

/// Reconcile discovered devices against command-line params, failing closed:
/// every device must have params and every param a device.
fn plan_mounts<'a>(
    params: &'a [(String, VerityParams)],
    devices: &'a [(String, String)],
) -> Vec<(&'a str, &'a str, &'a VerityParams)> {
    for (name, _) in params {
        if !devices.iter().any(|(dev_name, _)| dev_name == name) {
            panic!(
                "extension {name}: verity params on cmdline but no {EXTENSION_PREFIX}{name} device"
            );
        }
    }

    devices
        .iter()
        .map(|(name, dev)| {
            let verity = params
                .iter()
                .find(|(param_name, _)| param_name == name)
                .map(|(_, verity)| verity)
                .unwrap_or_else(|| {
                    panic!("extension {name}: device present but no verity params; refusing unverified mount")
                });
            (name.as_str(), dev.as_str(), verity)
        })
        .collect()
}

fn mount_extension(name: &str, dev: &str, params: &VerityParams) {
    let (data, hash) = find_partitions(SYS_BLOCK, dev);

    let dm_name = format!("{EXTENSION_PREFIX}{name}");
    let args = verity_args(&dm_name, &data, &hash, params);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    foreground(VERITYSETUP, &arg_refs);

    let mapper = format!("/dev/mapper/{dm_name}");
    let target = format!("{MOUNT_BASE}/{name}");
    fs::create_dir_all(&target).or_panic(format_args!("create_dir_all {target}"));

    // No NOEXEC: extensions ship executables (e.g. attestation-agent).
    let flags = MsFlags::MS_RDONLY | MsFlags::MS_NOSUID | MsFlags::MS_NODEV;
    nix::mount::mount(
        Some(mapper.as_str()),
        target.as_str(),
        Some("erofs"),
        flags,
        None::<&str>,
    )
    .or_panic(format_args!(
        "mount extension {name} ({mapper}) on {target}"
    ));

    info!("mounted extension {name} at {target}");
}

/// Parse `kata.extension.<name>.verity_params` entries into `(name, params)`.
fn parse_extensions(cmdline: &str) -> Vec<(String, VerityParams)> {
    cmdline
        .split_whitespace()
        .filter_map(|param| param.split_once('='))
        .filter_map(|(key, value)| {
            let name = key
                .strip_prefix(CMDLINE_KEY_PREFIX)?
                .strip_suffix(CMDLINE_KEY_SUFFIX)?;
            Some((name.to_owned(), parse_verity_params(name, value)))
        })
        .collect()
}

/// All five fields are required; missing or malformed is fatal (fail closed).
fn parse_verity_params(name: &str, value: &str) -> VerityParams {
    let mut root_hash = None;
    let mut salt = None;
    let mut data_blocks = None;
    let mut data_block_size = None;
    let mut hash_block_size = None;

    for (key, val) in value.split(',').filter_map(|kv| kv.split_once('=')) {
        match key {
            "root_hash" => root_hash = Some(val.to_owned()),
            "salt" => salt = Some(val.to_owned()),
            "data_blocks" => data_blocks = Some(parse_u64(name, "data_blocks", val)),
            "data_block_size" => data_block_size = Some(parse_u64(name, "data_block_size", val)),
            "hash_block_size" => hash_block_size = Some(parse_u64(name, "hash_block_size", val)),
            _ => {}
        }
    }

    let require = |field: &str, v: Option<String>| {
        v.unwrap_or_else(|| panic!("extension {name}: verity_params missing {field}"))
    };
    let require_num = |field: &str, v: Option<u64>| {
        v.filter(|n| *n != 0)
            .unwrap_or_else(|| panic!("extension {name}: verity_params missing or zero {field}"))
    };

    VerityParams {
        root_hash: require("root_hash", root_hash),
        salt: require("salt", salt),
        data_blocks: require_num("data_blocks", data_blocks),
        data_block_size: require_num("data_block_size", data_block_size),
        hash_block_size: require_num("hash_block_size", hash_block_size),
    }
}

fn parse_u64(name: &str, field: &str, value: &str) -> u64 {
    value
        .parse()
        .unwrap_or_else(|_| panic!("extension {name}: invalid verity_params {field}={value}"))
}

/// `veritysetup open` args, passed explicitly because the image is built
/// `--no-superblock`.
fn verity_args(dm_name: &str, data: &str, hash: &str, p: &VerityParams) -> Vec<String> {
    vec![
        "open".to_owned(),
        "--no-superblock".to_owned(),
        "--hash".to_owned(),
        "sha256".to_owned(),
        "--data-block-size".to_owned(),
        p.data_block_size.to_string(),
        "--hash-block-size".to_owned(),
        p.hash_block_size.to_string(),
        "--data-blocks".to_owned(),
        p.data_blocks.to_string(),
        "--salt".to_owned(),
        p.salt.clone(),
        data.to_owned(),
        dm_name.to_owned(),
        hash.to_owned(),
        p.root_hash.clone(),
    ]
}

/// Discover extension devices by `extension-<name>` serial into `(name, dev)`
/// pairs (e.g. `("coco", "vdb")`). Serial-based, so order-independent and no udev.
fn discover_extensions(sys_block: &str) -> Vec<(String, String)> {
    let Ok(entries) = fs::read_dir(sys_block) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let serial = fs::read_to_string(entry.path().join("serial")).ok()?;
            let name = serial.trim().strip_prefix(EXTENSION_PREFIX)?;
            // Name is a path component under MOUNT_BASE: reject traversal/bad chars.
            if name.is_empty()
                || name.bytes().all(|b| b == b'.')
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
            {
                panic!("invalid extension name {name:?}");
            }
            Some((
                name.to_owned(),
                entry.file_name().to_string_lossy().into_owned(),
            ))
        })
        .collect()
}

/// Data (partition 1) and hash (partition 2) paths, read from sysfs so the
/// naming convention (`vdb1` vs `nvme0n1p1`) does not matter.
fn find_partitions(sys_block: &str, dev: &str) -> (String, String) {
    let dir = format!("{sys_block}/{dev}");
    let mut data = None;
    let mut hash = None;

    for entry in fs::read_dir(&dir)
        .or_panic(format_args!("read_dir {dir}"))
        .flatten()
    {
        let Ok(number) = fs::read_to_string(entry.path().join("partition")) else {
            continue;
        };
        let part = format!("/dev/{}", entry.file_name().to_string_lossy());
        match number.trim() {
            "1" => data = Some(part),
            "2" => hash = Some(part),
            _ => {}
        }
    }

    (
        data.unwrap_or_else(|| panic!("extension device {dev}: missing data partition (1)")),
        hash.unwrap_or_else(|| panic!("extension device {dev}: missing hash partition (2)")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::{fixture, rstest};
    use tempfile::TempDir;

    const PARAMS: &str = "root_hash=abc123,salt=def456,data_blocks=96512,\
                          data_block_size=4096,hash_block_size=4096";

    // === parse_verity_params ===

    #[test]
    fn test_parse_verity_params_valid() {
        let p = parse_verity_params("coco", PARAMS);
        assert_eq!(p.root_hash, "abc123");
        assert_eq!(p.salt, "def456");
        assert_eq!(p.data_blocks, 96512);
        assert_eq!(p.data_block_size, 4096);
        assert_eq!(p.hash_block_size, 4096);
    }

    #[test]
    fn test_parse_verity_params_ignores_unknown_keys() {
        let p = parse_verity_params("coco", &format!("{PARAMS},extra=ignored"));
        assert_eq!(p.root_hash, "abc123");
    }

    #[rstest]
    #[case::missing_root_hash("salt=def,data_blocks=1,data_block_size=4096,hash_block_size=4096")]
    #[case::zero_data_blocks(
        "root_hash=a,salt=b,data_blocks=0,data_block_size=4096,hash_block_size=4096"
    )]
    #[case::non_numeric(
        "root_hash=a,salt=b,data_blocks=lots,data_block_size=4096,hash_block_size=4096"
    )]
    #[should_panic]
    fn test_parse_verity_params_invalid(#[case] params: &str) {
        parse_verity_params("coco", params);
    }

    // === parse_extensions ===

    #[rstest]
    #[case::single(
        format!("ro quiet kata.extension.coco.verity_params={PARAMS} console=ttyS0"),
        vec!["coco"]
    )]
    #[case::multiple(
        format!("kata.extension.coco.verity_params={PARAMS} kata.extension.gpu.verity_params={PARAMS}"),
        vec!["coco", "gpu"]
    )]
    #[case::none("ro quiet console=ttyS0 nvrc.log=debug".to_owned(), vec![])]
    #[case::other_kata_params("kata.extension.coco.other=x kata.something=y".to_owned(), vec![])]
    fn test_parse_extensions(#[case] cmdline: String, #[case] expected: Vec<&str>) {
        let extensions = parse_extensions(&cmdline);
        let names: Vec<&str> = extensions.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, expected);
    }

    // === verity_args ===

    #[test]
    fn test_verity_args_order() {
        let p = parse_verity_params("coco", PARAMS);
        let args = verity_args("extension-coco", "/dev/vdb1", "/dev/vdb2", &p);
        assert_eq!(
            args,
            vec![
                "open",
                "--no-superblock",
                "--hash",
                "sha256",
                "--data-block-size",
                "4096",
                "--hash-block-size",
                "4096",
                "--data-blocks",
                "96512",
                "--salt",
                "def456",
                "/dev/vdb1",
                "extension-coco",
                "/dev/vdb2",
                "abc123",
            ]
        );
    }

    // === discover_extensions ===

    fn write_serial(sys_block: &TempDir, dev: &str, serial: &str) {
        let dir = sys_block.path().join(dev);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("serial"), format!("{serial}\n")).unwrap();
    }

    /// sysfs with a rootfs device and a coco extension device exposing serials.
    #[fixture]
    fn sys_block() -> TempDir {
        let dir = TempDir::new().unwrap();
        write_serial(&dir, "vda", "rootfs");
        write_serial(&dir, "vdb", "extension-coco");
        dir
    }

    #[rstest]
    fn test_discover_extensions(sys_block: TempDir) {
        let found = discover_extensions(sys_block.path().to_str().unwrap());
        assert_eq!(found, vec![("coco".to_owned(), "vdb".to_owned())]);
    }

    #[test]
    fn test_discover_extensions_none() {
        let sys_block = TempDir::new().unwrap();
        write_serial(&sys_block, "vda", "rootfs");
        assert!(discover_extensions(sys_block.path().to_str().unwrap()).is_empty());
    }

    #[test]
    fn test_discover_extensions_missing_serial_file() {
        let sys_block = TempDir::new().unwrap();
        // device dir without a serial attribute must be skipped, not panic
        fs::create_dir_all(sys_block.path().join("vdb")).unwrap();
        assert!(discover_extensions(sys_block.path().to_str().unwrap()).is_empty());
    }

    #[test]
    fn test_discover_extensions_nonexistent_dir() {
        assert!(discover_extensions("/nonexistent/path").is_empty());
    }

    #[test]
    fn test_discover_extensions_allows_safe_name_chars() {
        let sys_block = TempDir::new().unwrap();
        write_serial(&sys_block, "vdb", "extension-gpu.v2_0-rc1");
        let found = discover_extensions(sys_block.path().to_str().unwrap());
        assert_eq!(found, vec![("gpu.v2_0-rc1".to_owned(), "vdb".to_owned())]);
    }

    #[test]
    #[should_panic]
    fn test_discover_extensions_empty_name_panics() {
        let sys_block = TempDir::new().unwrap();
        write_serial(&sys_block, "vdb", "extension-");
        discover_extensions(sys_block.path().to_str().unwrap());
    }

    #[test]
    #[should_panic]
    fn test_discover_extensions_invalid_name_panics() {
        let sys_block = TempDir::new().unwrap();
        // A path separator in the name could escape MOUNT_BASE: must fail closed.
        write_serial(&sys_block, "vdb", "extension-../evil");
        discover_extensions(sys_block.path().to_str().unwrap());
    }

    #[test]
    #[should_panic]
    fn test_discover_extensions_dots_only_name_panics() {
        let sys_block = TempDir::new().unwrap();
        // `extension-..` -> name `..` -> target `/run/kata-extensions/..` == `/run`.
        write_serial(&sys_block, "vdb", "extension-..");
        discover_extensions(sys_block.path().to_str().unwrap());
    }

    // === mount_all (no-op reconciliation) ===

    #[test]
    fn test_mount_all_noop_without_extensions() {
        // With no kata.extension.* params and no extension- devices, mount_all()
        // reconciles to an empty plan and mounts nothing. Guarded so it never
        // attempts a real mount on a host that does have extensions configured.
        let cmdline = fs::read_to_string(CMDLINE).unwrap_or_default();
        if !parse_extensions(&cmdline).is_empty() || !discover_extensions(SYS_BLOCK).is_empty() {
            return;
        }
        mount_all();
    }

    // === plan_mounts (fail-closed reconciliation) ===

    fn params(names: &[&str]) -> Vec<(String, VerityParams)> {
        names
            .iter()
            .map(|n| (n.to_string(), parse_verity_params(n, PARAMS)))
            .collect()
    }

    fn devices(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(n, d)| (n.to_string(), d.to_string()))
            .collect()
    }

    #[test]
    fn test_plan_mounts_matched() {
        let p = params(&["coco"]);
        let d = devices(&[("coco", "vdb")]);
        let plan = plan_mounts(&p, &d);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].0, "coco");
        assert_eq!(plan[0].1, "vdb");
    }

    #[test]
    fn test_plan_mounts_empty() {
        assert!(plan_mounts(&[], &[]).is_empty());
    }

    #[test]
    #[should_panic]
    fn test_plan_mounts_param_without_device() {
        plan_mounts(&params(&["coco"]), &[]);
    }

    #[test]
    #[should_panic]
    fn test_plan_mounts_device_without_param() {
        plan_mounts(&[], &devices(&[("coco", "vdb")]));
    }

    // === find_partitions ===

    fn write_partition(sys_block: &TempDir, dev: &str, part: &str, number: &str) {
        let dir = sys_block.path().join(dev).join(part);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("partition"), format!("{number}\n")).unwrap();
    }

    #[test]
    fn test_find_partitions() {
        // Extensions are cold-plugged as virtio-blk devices (vdX).
        let sys_block = TempDir::new().unwrap();
        write_partition(&sys_block, "vdb", "vdb1", "1");
        write_partition(&sys_block, "vdb", "vdb2", "2");
        // a non-partition sysfs attribute alongside partitions must be ignored
        fs::write(sys_block.path().join("vdb").join("size"), "100\n").unwrap();

        let (data, hash) = find_partitions(sys_block.path().to_str().unwrap(), "vdb");
        assert_eq!(data, "/dev/vdb1");
        assert_eq!(hash, "/dev/vdb2");
    }

    #[test]
    #[should_panic]
    fn test_find_partitions_missing_hash_panics() {
        let sys_block = TempDir::new().unwrap();
        write_partition(&sys_block, "vdb", "vdb1", "1");
        find_partitions(sys_block.path().to_str().unwrap(), "vdb");
    }

    #[test]
    #[should_panic]
    fn test_find_partitions_missing_data_panics() {
        let sys_block = TempDir::new().unwrap();
        write_partition(&sys_block, "vdb", "vdb2", "2");
        find_partitions(sys_block.path().to_str().unwrap(), "vdb");
    }
}
