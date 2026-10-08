// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Generic KEY=VALUE configuration file utilities.

use crate::macros::ResultExt;
use log::debug;
use std::collections::HashSet;
use std::fs;

/// Updates KEY=VALUE pairs in a config file, adding them if missing.
/// Existing keys are updated in place, new keys are appended to the end.
pub fn update_config_file(path: &str, updates: &[(&str, &str)]) {
    let content = fs::read_to_string(path).or_panic(format_args!("read {path}"));

    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    let mut found_keys: HashSet<&str> = HashSet::new();

    // Update existing lines
    for line in &mut lines {
        let trimmed = line.trim();
        for (key, value) in updates {
            if trimmed.starts_with(&format!("{}=", key)) {
                *line = format!("{}={}", key, value);
                found_keys.insert(key);
                debug!("{}: {}={}", path, key, value);
                break;
            }
        }
    }

    // Add missing keys
    for (key, value) in updates {
        if !found_keys.contains(key) {
            lines.push(format!("{}={}", key, value));
            debug!("{}: {}={}", path, key, value);
        }
    }

    let updated = lines.join("\n") + "\n";
    fs::write(path, updated).or_panic(format_args!("write {path}"));
}

/// Removes active KEY=VALUE lines for `keys` from a config file.
/// Commented-out lines are kept.
pub fn remove_config_keys(path: &str, keys: &[&str]) {
    let content = fs::read_to_string(path).or_panic(format_args!("read {path}"));

    let lines: Vec<&str> = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            let remove = keys
                .iter()
                .any(|key| trimmed.starts_with(&format!("{}=", key)));
            if remove {
                debug!("{}: removed {}", path, trimmed);
            }
            !remove
        })
        .collect();

    let updated = lines.join("\n") + "\n";
    fs::write(path, updated).or_panic(format_args!("write {path}"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::NamedTempFile;

    #[test]
    fn test_update_config_file_add_new_keys() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_str().unwrap();

        // Start with empty file
        fs::write(path, "").unwrap();

        update_config_file(path, &[("KEY1", "value1"), ("KEY2", "value2")]);

        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("KEY1=value1"));
        assert!(content.contains("KEY2=value2"));
    }

    #[test]
    fn test_update_config_file_update_existing_keys() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_str().unwrap();

        // Start with existing content
        fs::write(path, "KEY1=oldvalue\nKEY2=oldvalue\n").unwrap();

        update_config_file(path, &[("KEY1", "newvalue"), ("KEY2", "newvalue")]);

        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("KEY1=newvalue"));
        assert!(content.contains("KEY2=newvalue"));
        assert!(!content.contains("oldvalue"));
    }

    #[test]
    fn test_update_config_file_mixed_update_and_add() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_str().unwrap();

        // Start with one existing key
        fs::write(path, "KEY1=oldvalue\n").unwrap();

        update_config_file(path, &[("KEY1", "updated"), ("KEY2", "new")]);

        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("KEY1=updated"));
        assert!(content.contains("KEY2=new"));
        assert!(!content.contains("oldvalue"));
    }

    #[test]
    fn test_update_config_file_preserves_other_lines() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_str().unwrap();

        // Start with mixed content
        fs::write(path, "# Comment\nKEY1=old\nOTHER=unchanged\n").unwrap();

        update_config_file(path, &[("KEY1", "new")]);

        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("# Comment"));
        assert!(content.contains("KEY1=new"));
        assert!(content.contains("OTHER=unchanged"));
    }

    #[test]
    fn test_update_config_file_with_spaces() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_str().unwrap();

        fs::write(path, "  KEY1=old  \n").unwrap();

        update_config_file(path, &[("KEY1", "new")]);

        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("KEY1=new"));
    }

    #[test]
    fn test_update_config_file_empty_value() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_str().unwrap();

        fs::write(path, "").unwrap();

        update_config_file(path, &[("KEY1", "")]);

        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("KEY1="));
    }

    #[test]
    fn test_update_config_file_multiple_updates_same_key() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_str().unwrap();

        fs::write(path, "KEY1=old\n").unwrap();

        // Update twice
        update_config_file(path, &[("KEY1", "first")]);
        update_config_file(path, &[("KEY1", "second")]);

        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("KEY1=second"));
        assert!(!content.contains("first"));
    }

    #[test]
    fn test_update_config_file_similar_key_names() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_str().unwrap();

        // Test that FABRIC_MODE_RESTART doesn't match FABRIC_MODE
        fs::write(path, "FABRIC_MODE=0\nFABRIC_MODE_RESTART=0\n").unwrap();

        update_config_file(path, &[("FABRIC_MODE", "1")]);

        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("FABRIC_MODE=1"));
        assert!(content.contains("FABRIC_MODE_RESTART=0"));
    }

    #[test]
    #[should_panic(expected = "read")]
    fn test_update_config_file_nonexistent_file() {
        update_config_file("/nonexistent/path/file.cfg", &[("KEY", "value")]);
    }

    #[test]
    fn test_remove_config_keys() {
        let tmpfile = NamedTempFile::new().unwrap();
        let path = tmpfile.path().to_str().unwrap();

        fs::write(
            path,
            "# KEY1=commented\nKEY1=value\n  KEY1=indented\nKEY1_SUFFIX=kept\nOTHER=kept\n",
        )
        .unwrap();

        remove_config_keys(path, &["KEY1"]);

        let content = fs::read_to_string(path).unwrap();
        assert_eq!(content, "# KEY1=commented\nKEY1_SUFFIX=kept\nOTHER=kept\n");
    }

    #[test]
    #[should_panic(expected = "read")]
    fn test_remove_config_keys_nonexistent_file() {
        remove_config_keys("/nonexistent/path/file.cfg", &["KEY"]);
    }
}
