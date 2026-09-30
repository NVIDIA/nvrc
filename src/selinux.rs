// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Load the guest SELinux policy shipped in the `selinux` extension.
//!
//! The kernel rejects `context=` until a policy is loaded, so this runs before
//! any other extension is mounted.
//!
//!   selinux=1, no selinux extension    -> panic (fail closed)
//!   selinux=1, kernel without SELinux  -> panic (fail closed)
//!   selinux extension, SELinux enabled -> load policy, lock policy reloads
//!   policy without the lock boolean    -> panic (fail closed)
//!   otherwise                          -> no-op, extensions mount unlabeled

use log::info;
use nix::mount::MsFlags;
use std::fs;
use std::path::Path;

use crate::macros::ResultExt;
use crate::mount::fs_available;

pub(crate) const EXTENSION: &str = "selinux";

const SELINUXFS: &str = "/sys/fs/selinux";
const POLICY: &str = "etc/selinux/kata/policy";
const EXTENSION_CONTEXTS: &str = "etc/selinux/kata/extension_contexts";
/// Nothing in the guest, NVRC included, may change the policy once it is loaded.
const LOCKDOWN_BOOLEAN: &str = "secure_mode_policyload";
const WILDCARD: &str = "*";

#[derive(Debug, Default, PartialEq)]
pub struct MountContexts(Vec<(String, String)>);

impl MountContexts {
    pub fn mount_data(&self, name: &str) -> Option<String> {
        let lookup = |key: &str| self.0.iter().find(|(n, _)| n == key).map(|(_, c)| c);
        // Quoted: MCS levels such as `s0:c1,c2` contain commas.
        lookup(name)
            .or_else(|| lookup(WILDCARD))
            .map(|context| format!("context=\"{context}\""))
    }
}

pub fn setup(cmdline: &str, extension_root: Option<&str>) -> MountContexts {
    let filesystems = fs::read_to_string("/proc/filesystems").or_panic("read /proc/filesystems");
    setup_at(
        cmdline,
        fs_available(&filesystems, "selinuxfs"),
        extension_root,
        SELINUXFS,
    )
}

fn setup_at(
    cmdline: &str,
    kernel_enabled: bool,
    extension_root: Option<&str>,
    selinuxfs: &str,
) -> MountContexts {
    let requested = requested_on_cmdline(cmdline);
    let Some(root) = extension_root else {
        if requested {
            panic!("selinux=1 but no {EXTENSION} extension carrying the guest policy");
        }
        return MountContexts::default();
    };
    if !kernel_enabled {
        if requested {
            panic!("selinux=1 but the guest kernel has SELinux disabled");
        }
        info!("SELinux disabled in the guest kernel, not loading the guest policy");
        return MountContexts::default();
    }

    mount_selinuxfs(selinuxfs);
    let policy = format!("{root}/{POLICY}");
    let data = fs::read(&policy).or_panic(format_args!("read {policy}"));
    fs::write(format!("{selinuxfs}/load"), &data).or_panic(format_args!("load {policy}"));
    info!("loaded guest SELinux policy ({} bytes)", data.len());
    lock_policy(selinuxfs);

    let contexts = format!("{root}/{EXTENSION_CONTEXTS}");
    MountContexts(parse_extension_contexts(
        &fs::read_to_string(&contexts).or_panic(format_args!("read {contexts}")),
    ))
}

/// The kernel honours the last `selinux=`.
fn requested_on_cmdline(cmdline: &str) -> bool {
    cmdline
        .split_whitespace()
        .filter_map(|param| param.strip_prefix("selinux="))
        .next_back()
        == Some("1")
}

fn mount_selinuxfs(target: &str) {
    if Path::new(&format!("{target}/load")).exists() {
        return;
    }
    let flags = MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC | MsFlags::MS_RELATIME;
    nix::mount::mount(
        Some("selinuxfs"),
        target,
        Some("selinuxfs"),
        flags,
        None::<&str>,
    )
    .or_panic(format_args!("mount selinuxfs on {target}"));
}

fn lock_policy(selinuxfs: &str) {
    let boolean = format!("{selinuxfs}/booleans/{LOCKDOWN_BOOLEAN}");
    fs::write(&boolean, "1").or_panic(format_args!("set {LOCKDOWN_BOOLEAN}"));
    fs::write(format!("{selinuxfs}/commit_pending_bools"), "1")
        .or_panic(format_args!("commit {LOCKDOWN_BOOLEAN}"));
    info!("{LOCKDOWN_BOOLEAN} set: guest SELinux policy locked");
}

fn parse_extension_contexts(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let mut fields = line.split_whitespace();
            match (fields.next(), fields.next(), fields.next()) {
                (Some(name), Some(context), None) if context.split(':').count() >= 4 => {
                    (name.to_owned(), context.to_owned())
                }
                _ => panic!("invalid extension_contexts line {line:?}"),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use tempfile::TempDir;

    const GPU: &str = "system_u:object_r:kata_gpu_extension_t:s0";
    const DEFAULT: &str = "system_u:object_r:kata_extension_t:s0";

    #[rstest]
    #[case::enabled("ro selinux=1 quiet", true)]
    #[case::disabled("selinux=0", false)]
    #[case::absent("ro quiet", false)]
    #[case::last_wins("selinux=1 selinux=0", false)]
    #[case::last_wins_enable("selinux=0 selinux=1", true)]
    #[case::other_key("nvrc.selinux=1", false)]
    fn test_requested_on_cmdline(#[case] cmdline: &str, #[case] expected: bool) {
        assert_eq!(requested_on_cmdline(cmdline), expected);
    }

    #[test]
    fn test_parse_extension_contexts() {
        let text = format!("# comment\n\ngpu  {GPU}\n*\t{DEFAULT}\n");
        assert_eq!(
            parse_extension_contexts(&text),
            vec![
                ("gpu".to_owned(), GPU.to_owned()),
                ("*".to_owned(), DEFAULT.to_owned()),
            ]
        );
    }

    #[rstest]
    #[case::missing_context("gpu")]
    #[case::extra_field("gpu system_u:object_r:t:s0 extra")]
    #[case::not_a_context("gpu container_t")]
    #[should_panic]
    fn test_parse_extension_contexts_invalid(#[case] line: &str) {
        parse_extension_contexts(line);
    }

    #[test]
    fn test_mount_data() {
        let contexts = MountContexts(parse_extension_contexts(&format!(
            "gpu {GPU}\n* {DEFAULT}\n"
        )));
        assert_eq!(
            contexts.mount_data("gpu"),
            Some(format!("context=\"{GPU}\""))
        );
        assert_eq!(
            contexts.mount_data("devkit"),
            Some(format!("context=\"{DEFAULT}\""))
        );
    }

    #[test]
    fn test_mount_data_without_wildcard() {
        let contexts = MountContexts(parse_extension_contexts(&format!("gpu {GPU}\n")));
        assert_eq!(contexts.mount_data("coco"), None);
        assert_eq!(MountContexts::default().mount_data("gpu"), None);
    }

    #[test]
    fn test_setup_noop_without_extension() {
        assert_eq!(
            setup_at("ro quiet", true, None, "/nonexistent"),
            MountContexts::default()
        );
    }

    #[test]
    #[should_panic(expected = "no selinux extension")]
    fn test_setup_requested_without_extension_panics() {
        setup_at("selinux=1", true, None, "/nonexistent");
    }

    #[test]
    #[should_panic(expected = "SELinux disabled")]
    fn test_setup_requested_kernel_disabled_panics() {
        setup_at("selinux=1", false, Some("/nonexistent"), "/nonexistent");
    }

    #[test]
    fn test_setup_kernel_disabled_is_noop() {
        assert_eq!(
            setup_at("selinux=0", false, Some("/nonexistent"), "/nonexistent"),
            MountContexts::default()
        );
    }

    /// A pre-existing `load` file stops `mount_selinuxfs` from mounting.
    fn fake_tree() -> (TempDir, TempDir) {
        let ext = TempDir::new().unwrap();
        let kata = ext.path().join("etc/selinux/kata");
        fs::create_dir_all(&kata).unwrap();
        fs::write(kata.join("policy"), b"policy-bytes").unwrap();
        fs::write(kata.join("extension_contexts"), format!("gpu {GPU}\n")).unwrap();

        let selinuxfs = TempDir::new().unwrap();
        fs::write(selinuxfs.path().join("load"), b"").unwrap();
        let booleans = selinuxfs.path().join("booleans");
        fs::create_dir_all(&booleans).unwrap();
        fs::write(booleans.join(LOCKDOWN_BOOLEAN), "0").unwrap();
        fs::write(selinuxfs.path().join("commit_pending_bools"), "").unwrap();
        (ext, selinuxfs)
    }

    #[test]
    fn test_setup_loads_and_locks_policy() {
        let (ext, selinuxfs) = fake_tree();
        let sfs = selinuxfs.path().to_str().unwrap();

        let contexts = setup_at("selinux=1", true, ext.path().to_str(), sfs);

        assert_eq!(fs::read(format!("{sfs}/load")).unwrap(), b"policy-bytes");
        assert_eq!(
            contexts.mount_data("gpu"),
            Some(format!("context=\"{GPU}\""))
        );
        let boolean = format!("{sfs}/booleans/{LOCKDOWN_BOOLEAN}");
        assert_eq!(fs::read_to_string(boolean).unwrap(), "1");
        assert_eq!(
            fs::read_to_string(format!("{sfs}/commit_pending_bools")).unwrap(),
            "1"
        );
    }

    #[test]
    #[should_panic(expected = "set secure_mode_policyload")]
    fn test_setup_policy_without_lockdown_boolean_panics() {
        let (ext, selinuxfs) = fake_tree();
        fs::remove_dir_all(selinuxfs.path().join("booleans")).unwrap();
        setup_at(
            "selinux=1",
            true,
            ext.path().to_str(),
            selinuxfs.path().to_str().unwrap(),
        );
    }

    #[test]
    #[should_panic(expected = "read")]
    fn test_setup_missing_policy_panics() {
        let (ext, selinuxfs) = fake_tree();
        fs::remove_file(ext.path().join(POLICY)).unwrap();
        setup_at(
            "selinux=1",
            true,
            ext.path().to_str(),
            selinuxfs.path().to_str().unwrap(),
        );
    }
}
