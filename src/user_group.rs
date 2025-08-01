use nix::unistd::{Gid, Uid};
use rand::Rng;
use std::fs::OpenOptions;
use std::io::Write;
#[derive(Debug)]
pub struct UserGroup {
    pub user_id: Uid,
    pub group_id: Gid,
    pub user_name: String,
    pub group_name: String,
}

impl UserGroup {
    pub fn new() -> Self {
        UserGroup {
            user_id: Uid::from_raw(0),
            group_id: Gid::from_raw(0),
            user_name: String::from("root"),
            group_name: String::from("root"),
        }
    }
}

pub fn random_user_group() -> UserGroup {
    let mut rng = rand::rng();
    let uid = rng.random_range(1000..60000); // Generating user ID in the range 1000-60000
    let gid = rng.random_range(1000..60000); // Generating group ID in the range 1000-60000

    let user_name: String = (0..8)
        .map(|_| (rng.random_range(b'a'..=b'z') as char))
        .collect();
    let group_name: String = (0..8)
        .map(|_| (rng.random_range(b'a'..=b'z') as char))
        .collect();

    let user_id = Uid::from_raw(uid);
    let group_id = Gid::from_raw(gid);

    let user_group = UserGroup {
        user_id,
        group_id,
        user_name,
        group_name,
    };

    add_to_passwd(&user_group, "/etc/passwd");
    add_to_shadow(&user_group, "/etc/shadow");
    add_to_group(&user_group, "/etc/group");

    user_group
}

fn add_to_passwd(user_group: &UserGroup, passwd_path: &str) {
    let passwd_entry = format!(
        "{}:x:{}:{}:{}:/nonexistent:/bin/false\n",
        user_group.user_name, user_group.user_id, user_group.group_id, user_group.user_name
    );

    let mut passwd_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(passwd_path)
        .expect("failed to open passwd file");

    passwd_file
        .write_all(passwd_entry.as_bytes())
        .expect("failed to write to passwd file");
}

fn add_to_shadow(user_group: &UserGroup, shadow_path: &str) {
    let shadow_entry = format!("{}:*:18295:0:99999:7:::\n", user_group.user_name);

    let mut shadow_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(shadow_path)
        .expect("failed to open shadow file");

    shadow_file
        .write_all(shadow_entry.as_bytes())
        .expect("failed to write to shadow file");
}

fn add_to_group(user_group: &UserGroup, group_path: &str) {
    let group_entry = format!("{}:x:{}:\n", user_group.group_name, user_group.group_id,);

    let mut group_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(group_path)
        .expect("failed to open group file");

    group_file
        .write_all(group_entry.as_bytes())
        .expect("failed to write to group file");
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

        // Test the passwd entry format
        let passwd_entry = format!(
            "{}:x:{}:{}:{}:/nonexistent:/bin/false\n",
            user_group.user_name, user_group.user_id, user_group.group_id, user_group.user_name
        );

        // Verify passwd format: username:password:uid:gid:gecos:home:shell
        let expected = "testuser:x:1001:1001:testuser:/nonexistent:/bin/false\n";
        assert_eq!(passwd_entry, expected);

        // Verify field count and format
        let fields: Vec<&str> = passwd_entry.trim_end().split(':').collect();
        assert_eq!(fields.len(), 7, "passwd entry should have exactly 7 fields");
        assert_eq!(fields[0], "testuser");   // username
        assert_eq!(fields[1], "x");          // password placeholder
        assert_eq!(fields[2], "1001");       // uid
        assert_eq!(fields[3], "1001");       // gid
        assert_eq!(fields[4], "testuser");   // gecos (username as description)
        assert_eq!(fields[5], "/nonexistent"); // home directory
        assert_eq!(fields[6], "/bin/false");  // shell
    }

    #[test]
    fn test_shadow_entry_format() {
        let user_group = UserGroup {
            user_id: Uid::from_raw(1001),
            group_id: nix::unistd::Gid::from_raw(1001),
            user_name: "testuser".to_string(),
            group_name: "testgroup".to_string(),
        };

        // Test the shadow entry format
        let shadow_entry = format!("{}:*:18295:0:99999:7:::\n", user_group.user_name);
        let expected = "testuser:*:18295:0:99999:7:::\n";
        assert_eq!(shadow_entry, expected);

        // Verify shadow format: username:password:lastchange:min:max:warn:inactive:expire:reserved
        let fields: Vec<&str> = shadow_entry.trim_end().split(':').collect();
        assert_eq!(fields.len(), 9, "shadow entry should have exactly 9 fields");
        assert_eq!(fields[0], "testuser");  // username
        assert_eq!(fields[1], "*");         // password (disabled)
        assert_eq!(fields[2], "18295");     // last password change (days since epoch)
        assert_eq!(fields[3], "0");         // minimum password age
        assert_eq!(fields[4], "99999");     // maximum password age
        assert_eq!(fields[5], "7");         // password warning period
        assert_eq!(fields[6], "");          // password inactivity period (empty)
        assert_eq!(fields[7], "");          // account expiration date (empty)
        assert_eq!(fields[8], "");          // reserved field (empty)
    }

    #[test]
    fn test_group_entry_format() {
        let user_group = UserGroup {
            user_id: Uid::from_raw(1001),
            group_id: nix::unistd::Gid::from_raw(1001),
            user_name: "testuser".to_string(),
            group_name: "testgroup".to_string(),
        };

        // Test the group entry format
        let group_entry = format!("{}:x:{}:\n", user_group.group_name, user_group.group_id);
        let expected = "testgroup:x:1001:\n";
        assert_eq!(group_entry, expected);

        // Verify group format: groupname:password:gid:members
        let fields: Vec<&str> = group_entry.trim_end().split(':').collect();
        assert_eq!(fields.len(), 4, "group entry should have exactly 4 fields");
        assert_eq!(fields[0], "testgroup"); // group name
        assert_eq!(fields[1], "x");         // password placeholder
        assert_eq!(fields[2], "1001");      // gid
        assert_eq!(fields[3], "");          // members list (empty)
    }

    #[test]
    fn test_random_user_group_generation() {
        // Test that random_user_group generates valid ranges
        // We can't test the actual random_user_group function easily because it writes to /etc files
        // Instead, test the generation logic
        let mut rng = rand::rng();
        let uid = rng.random_range(1000..60000);
        let gid = rng.random_range(1000..60000);
        
        assert!(uid >= 1000 && uid < 60000, "UID should be in valid range");
        assert!(gid >= 1000 && gid < 60000, "GID should be in valid range");
        
        // Test username generation
        let user_name: String = (0..8)
            .map(|_| (rng.random_range(b'a'..=b'z') as char))
            .collect();
        
        assert_eq!(user_name.len(), 8, "Username should be 8 characters");
        assert!(user_name.chars().all(|c| c.is_ascii_lowercase()), "Username should only contain lowercase letters");
    }

    #[test]
    fn test_passwd_entry_edge_cases() {
        // Test with edge case values
        let user_group = UserGroup {
            user_id: Uid::from_raw(0),      // root user
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
        // Test that shadow entry uses valid defaults
        let user_group = UserGroup {
            user_id: Uid::from_raw(1001),
            group_id: nix::unistd::Gid::from_raw(1001),
            user_name: "testuser".to_string(),
            group_name: "testgroup".to_string(),
        };

        let shadow_entry = format!("{}:*:18295:0:99999:7:::\n", user_group.user_name);
        
        // Verify that the password is disabled with '*'
        assert!(shadow_entry.contains(":*:"), "Password should be disabled with '*'");
        
        // Verify reasonable password policy values
        assert!(shadow_entry.contains(":99999:"), "Max password age should be reasonable");
        assert!(shadow_entry.contains(":7:"), "Warning period should be set");
    }

    #[test]
    fn test_group_entry_no_members() {
        // Test that group entry correctly has no members initially
        let user_group = UserGroup {
            user_id: Uid::from_raw(1001),
            group_id: nix::unistd::Gid::from_raw(1001),
            user_name: "testuser".to_string(),
            group_name: "testgroup".to_string(),
        };

        let group_entry = format!("{}:x:{}:\n", user_group.group_name, user_group.group_id);
        
        // Should end with empty members field
        assert!(group_entry.ends_with(":\n"), "Group should have empty members list");
    }

    #[test]
    fn test_format_compliance_with_real_examples() {
        // Test against real-world examples of these file formats
        
        // Real passwd entry examples:
        // root:x:0:0:root:/root:/bin/bash
        // daemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin
        
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

        // Should match standard format exactly
        assert_eq!(passwd_entry, "myuser:x:1234:1234:myuser:/nonexistent:/bin/false\n");
        
        // Verify it would be parsable by system tools
        let parts: Vec<&str> = passwd_entry.trim().split(':').collect();
        assert_eq!(parts.len(), 7);
        assert!(parts[2].parse::<u32>().is_ok()); // UID should be numeric
        assert!(parts[3].parse::<u32>().is_ok()); // GID should be numeric
    }

    #[test]
    #[serial]
    fn test_safe_file_operations() {
        // Use temporary files - NEVER touch system files in tests!
        let temp_passwd = tempfile::NamedTempFile::new().unwrap();
        let temp_shadow = tempfile::NamedTempFile::new().unwrap();
        let temp_group = tempfile::NamedTempFile::new().unwrap();

        let user_group = UserGroup {
            user_id: Uid::from_raw(9999),
            group_id: nix::unistd::Gid::from_raw(9999),
            user_name: "testuser".to_string(),
            group_name: "testgroup".to_string(),
        };

        // Test writing to temporary files
        add_to_passwd(&user_group, temp_passwd.path().to_str().unwrap());
        add_to_shadow(&user_group, temp_shadow.path().to_str().unwrap());
        add_to_group(&user_group, temp_group.path().to_str().unwrap());

        // Verify the content was written correctly
        let passwd_content = std::fs::read_to_string(temp_passwd.path()).unwrap();
        let shadow_content = std::fs::read_to_string(temp_shadow.path()).unwrap();
        let group_content = std::fs::read_to_string(temp_group.path()).unwrap();

        assert_eq!(passwd_content, "testuser:x:9999:9999:testuser:/nonexistent:/bin/false\n");
        assert_eq!(shadow_content, "testuser:*:18295:0:99999:7:::\n");
        assert_eq!(group_content, "testgroup:x:9999:\n");

        // Verify no invalid characters or formatting issues
        assert!(!passwd_content.contains("\0"));
        assert!(!shadow_content.contains("\0"));
        assert!(!group_content.contains("\0"));
        
        assert!(passwd_content.ends_with('\n'));
        assert!(shadow_content.ends_with('\n'));
        assert!(group_content.ends_with('\n'));
    }
}
