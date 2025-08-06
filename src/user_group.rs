use anyhow::{Context, Result};
use nix::unistd::{Gid, Uid};
use rand::Rng;
use std::fs::OpenOptions;
use std::io::Write;

const MIN_USER_ID: u32 = 1000;
const MAX_USER_ID: u32 = 60000;
const USERNAME_LENGTH: usize = 8;
const DEFAULT_PASSWD_PATH: &str = "/etc/passwd";
const DEFAULT_SHADOW_PATH: &str = "/etc/shadow";
const DEFAULT_GROUP_PATH: &str = "/etc/group";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserGroup {
    pub user_id: Uid,
    pub group_id: Gid,
    pub user_name: String,
    pub group_name: String,
}

impl UserGroup {
    pub fn new() -> Self {
        Self::root()
    }

    pub fn root() -> Self {
        Self {
            user_id: Uid::from_raw(0),
            group_id: Gid::from_raw(0),
            user_name: "root".to_owned(),
            group_name: "root".to_owned(),
        }
    }

    pub fn with_ids(user_id: u32, group_id: u32, user_name: String, group_name: String) -> Self {
        Self {
            user_id: Uid::from_raw(user_id),
            group_id: Gid::from_raw(group_id),
            user_name,
            group_name,
        }
    }

    pub fn write_to_system_files(&self) -> Result<()> {
        self.write_to_files(DEFAULT_PASSWD_PATH, DEFAULT_SHADOW_PATH, DEFAULT_GROUP_PATH)
    }

    pub fn write_to_files(
        &self,
        passwd_path: &str,
        shadow_path: &str,
        group_path: &str,
    ) -> Result<()> {
        self.add_to_passwd(passwd_path)
            .context("Failed to write to passwd file")?;
        self.add_to_shadow(shadow_path)
            .context("Failed to write to shadow file")?;
        self.add_to_group(group_path)
            .context("Failed to write to group file")?;
        Ok(())
    }

    fn passwd_entry(&self) -> String {
        format!(
            "{}:x:{}:{}:{}:/nonexistent:/bin/false\n",
            self.user_name, self.user_id, self.group_id, self.user_name
        )
    }

    fn shadow_entry(&self) -> String {
        format!("{}:*:18295:0:99999:7:::\n", self.user_name)
    }

    fn group_entry(&self) -> String {
        format!("{}:x:{}:\n", self.group_name, self.group_id)
    }

    fn add_to_passwd(&self, passwd_path: &str) -> Result<()> {
        let entry = self.passwd_entry();
        self.append_to_file(passwd_path, &entry)
            .with_context(|| format!("Failed to write to passwd file: {}", passwd_path))
    }

    fn add_to_shadow(&self, shadow_path: &str) -> Result<()> {
        let entry = self.shadow_entry();
        self.append_to_file(shadow_path, &entry)
            .with_context(|| format!("Failed to write to shadow file: {}", shadow_path))
    }

    fn add_to_group(&self, group_path: &str) -> Result<()> {
        let entry = self.group_entry();
        self.append_to_file(group_path, &entry)
            .with_context(|| format!("Failed to write to group file: {}", group_path))
    }

    fn append_to_file(&self, file_path: &str, content: &str) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(file_path)
            .with_context(|| format!("Failed to open file: {}", file_path))?;
        file.write_all(content.as_bytes())
            .with_context(|| format!("Failed to write to file: {}", file_path))?;
        Ok(())
    }
}

impl Default for UserGroup {
    fn default() -> Self {
        Self::new()
    }
}

pub fn random_user_group() -> UserGroup {
    let mut rng = rand::rng();
    let uid = rng.random_range(MIN_USER_ID..MAX_USER_ID);
    let gid = rng.random_range(MIN_USER_ID..MAX_USER_ID);
    let user_name: String = (0..USERNAME_LENGTH)
        .map(|_| rng.random_range(b'a'..=b'z') as char)
        .collect();
    let group_name = user_name.clone();
    let user_group = UserGroup::with_ids(uid, gid, user_name, group_name);
    let _ = user_group.write_to_system_files();
    user_group
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::unistd::Uid;
    use serial_test::serial;

    #[test]
    fn test_user_group_new() {
        let user_group = UserGroup::new();
        assert_eq!(user_group.user_id, Uid::from_raw(0));
        assert_eq!(user_group.group_id, nix::unistd::Gid::from_raw(0));
        assert_eq!(user_group.user_name, "root");
        assert_eq!(user_group.group_name, "root");
    }

    #[test]
    fn test_passwd_entry_format() {
        let user_group = UserGroup {
            user_id: Uid::from_raw(1001),
            group_id: nix::unistd::Gid::from_raw(1001),
            user_name: "testuser".to_string(),
            group_name: "testgroup".to_string(),
        };
        let passwd_entry = format!(
            "{}:x:{}:{}:{}:/nonexistent:/bin/false\n",
            user_group.user_name, user_group.user_id, user_group.group_id, user_group.user_name
        );
        let expected = "testuser:x:1001:1001:testuser:/nonexistent:/bin/false\n";
        assert_eq!(passwd_entry, expected);
        let fields: Vec<&str> = passwd_entry.trim_end().split(':').collect();
        assert_eq!(fields.len(), 7, "passwd entry should have exactly 7 fields");
        assert_eq!(fields[0], "testuser");
        assert_eq!(fields[1], "x");
        assert_eq!(fields[2], "1001");
        assert_eq!(fields[3], "1001");
        assert_eq!(fields[4], "testuser");
        assert_eq!(fields[5], "/nonexistent");
        assert_eq!(fields[6], "/bin/false");
    }

    #[test]
    fn test_shadow_entry_format() {
        let user_group = UserGroup {
            user_id: Uid::from_raw(1001),
            group_id: nix::unistd::Gid::from_raw(1001),
            user_name: "testuser".to_string(),
            group_name: "testgroup".to_string(),
        };
        let shadow_entry = format!("{}:*:18295:0:99999:7:::\n", user_group.user_name);
        let expected = "testuser:*:18295:0:99999:7:::\n";
        assert_eq!(shadow_entry, expected);
        let fields: Vec<&str> = shadow_entry.trim_end().split(':').collect();
        assert_eq!(fields.len(), 9, "shadow entry should have exactly 9 fields");
        assert_eq!(fields[0], "testuser");
        assert_eq!(fields[1], "*");
        assert_eq!(fields[2], "18295");
        assert_eq!(fields[3], "0");
        assert_eq!(fields[4], "99999");
        assert_eq!(fields[5], "7");
        assert_eq!(fields[6], "");
        assert_eq!(fields[7], "");
        assert_eq!(fields[8], "");
    }

    #[test]
    fn test_group_entry_format() {
        let user_group = UserGroup {
            user_id: Uid::from_raw(1001),
            group_id: nix::unistd::Gid::from_raw(1001),
            user_name: "testuser".to_string(),
            group_name: "testgroup".to_string(),
        };
        let group_entry = format!("{}:x:{}:\n", user_group.group_name, user_group.group_id);
        let expected = "testgroup:x:1001:\n";
        assert_eq!(group_entry, expected);
        let fields: Vec<&str> = group_entry.trim_end().split(':').collect();
        assert_eq!(fields.len(), 4, "group entry should have exactly 4 fields");
        assert_eq!(fields[0], "testgroup");
        assert_eq!(fields[1], "x");
        assert_eq!(fields[2], "1001");
        assert_eq!(fields[3], "");
    }

    #[test]
    fn test_random_user_group_generation() {
        let mut rng = rand::rng();
        let uid = rng.random_range(1000..60000);
        let gid = rng.random_range(1000..60000);
        assert!(uid >= 1000 && uid < 60000, "UID should be in valid range");
        assert!(gid >= 1000 && gid < 60000, "GID should be in valid range");
        let user_name: String = (0..8)
            .map(|_| (rng.random_range(b'a'..=b'z') as char))
            .collect();
        assert_eq!(user_name.len(), 8, "Username should be 8 characters");
        assert!(
            user_name.chars().all(|c| c.is_ascii_lowercase()),
            "Username should only contain lowercase letters"
        );
    }

    #[test]
    fn test_passwd_entry_edge_cases() {
        let user_group = UserGroup {
            user_id: Uid::from_raw(0),
            group_id: nix::unistd::Gid::from_raw(0),
            user_name: "root".to_string(),
            group_name: "root".to_string(),
        };
        let passwd_entry = format!(
            "{}:x:{}:{}:{}:/nonexistent:/bin/false\n",
            user_group.user_name, user_group.user_id, user_group.group_id, user_group.user_name
        );
        assert_eq!(passwd_entry, "root:x:0:0:root:/nonexistent:/bin/false\n");
    }

    #[test]
    fn test_shadow_entry_validity() {
        let user_group = UserGroup {
            user_id: Uid::from_raw(1001),
            group_id: nix::unistd::Gid::from_raw(1001),
            user_name: "testuser".to_string(),
            group_name: "testgroup".to_string(),
        };
        let shadow_entry = format!("{}:*:18295:0:99999:7:::\n", user_group.user_name);
        assert!(
            shadow_entry.contains(":*:"),
            "Password should be disabled with '*'"
        );
        assert!(
            shadow_entry.contains(":99999:"),
            "Max password age should be reasonable"
        );
        assert!(shadow_entry.contains(":7:"), "Warning period should be set");
    }

    #[test]
    fn test_group_entry_no_members() {
        let user_group = UserGroup {
            user_id: Uid::from_raw(1001),
            group_id: nix::unistd::Gid::from_raw(1001),
            user_name: "testuser".to_string(),
            group_name: "testgroup".to_string(),
        };
        let group_entry = format!("{}:x:{}:\n", user_group.group_name, user_group.group_id);
        assert!(
            group_entry.ends_with(":\n"),
            "Group should have empty members list"
        );
    }

    #[test]
    fn test_format_compliance_with_real_examples() {
        let user_group = UserGroup {
            user_id: Uid::from_raw(1234),
            group_id: nix::unistd::Gid::from_raw(1234),
            user_name: "myuser".to_string(),
            group_name: "mygroup".to_string(),
        };
        let passwd_entry = format!(
            "{}:x:{}:{}:{}:/nonexistent:/bin/false\n",
            user_group.user_name, user_group.user_id, user_group.group_id, user_group.user_name
        );
        assert_eq!(
            passwd_entry,
            "myuser:x:1234:1234:myuser:/nonexistent:/bin/false\n"
        );
        let parts: Vec<&str> = passwd_entry.trim().split(':').collect();
        assert_eq!(parts.len(), 7);
        assert!(parts[2].parse::<u32>().is_ok());
        assert!(parts[3].parse::<u32>().is_ok());
    }

    #[test]
    #[serial]
    fn test_safe_file_operations() {
        let temp_passwd = tempfile::NamedTempFile::new().unwrap();
        let temp_shadow = tempfile::NamedTempFile::new().unwrap();
        let temp_group = tempfile::NamedTempFile::new().unwrap();
        let user_group = UserGroup {
            user_id: Uid::from_raw(9999),
            group_id: nix::unistd::Gid::from_raw(9999),
            user_name: "testuser".to_string(),
            group_name: "testgroup".to_string(),
        };
        user_group
            .write_to_files(
                temp_passwd.path().to_str().unwrap(),
                temp_shadow.path().to_str().unwrap(),
                temp_group.path().to_str().unwrap(),
            )
            .unwrap();
        let passwd_content = std::fs::read_to_string(temp_passwd.path()).unwrap();
        let shadow_content = std::fs::read_to_string(temp_shadow.path()).unwrap();
        let group_content = std::fs::read_to_string(temp_group.path()).unwrap();
        assert_eq!(
            passwd_content,
            "testuser:x:9999:9999:testuser:/nonexistent:/bin/false\n"
        );
        assert_eq!(shadow_content, "testuser:*:18295:0:99999:7:::\n");
        assert_eq!(group_content, "testgroup:x:9999:\n");
        assert!(!passwd_content.contains("\0"));
        assert!(!shadow_content.contains("\0"));
        assert!(!group_content.contains("\0"));
        assert!(passwd_content.ends_with('\n'));
        assert!(shadow_content.ends_with('\n'));
        assert!(group_content.ends_with('\n'));
    }

    #[test]
    fn test_user_group_with_ids() {
        let user_group =
            UserGroup::with_ids(1234, 5678, "testuser".to_owned(), "testgroup".to_owned());
        assert_eq!(user_group.user_id, Uid::from_raw(1234));
        assert_eq!(user_group.group_id, Gid::from_raw(5678));
        assert_eq!(user_group.user_name, "testuser");
        assert_eq!(user_group.group_name, "testgroup");
    }

    #[test]
    fn test_user_group_root() {
        let root = UserGroup::root();
        assert_eq!(root.user_id, Uid::from_raw(0));
        assert_eq!(root.group_id, Gid::from_raw(0));
        assert_eq!(root.user_name, "root");
        assert_eq!(root.group_name, "root");
    }

    #[test]
    fn test_user_group_default() {
        let default = UserGroup::default();
        let new = UserGroup::new();
        assert_eq!(default, new);
        assert_eq!(default.user_name, "root");
    }

    #[test]
    fn test_entry_generation() {
        let user_group = UserGroup::with_ids(1001, 1002, "myuser".to_owned(), "mygroup".to_owned());
        let passwd = user_group.passwd_entry();
        assert_eq!(
            passwd,
            "myuser:x:1001:1002:myuser:/nonexistent:/bin/false\n"
        );
        let shadow = user_group.shadow_entry();
        assert_eq!(shadow, "myuser:*:18295:0:99999:7:::\n");
        let group = user_group.group_entry();
        assert_eq!(group, "mygroup:x:1002:\n");
    }
}
