// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

pub fn character_major(devices: &str, name: &str) -> Option<u32> {
    devices
        .lines()
        .skip_while(|line| line.trim() != "Character devices:")
        .skip(1)
        .take_while(|line| line.trim() != "Block devices:")
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            let major = fields.next()?;
            (fields.next() == Some(name) && fields.next().is_none()).then_some(major)
        })
        .and_then(|major| major.parse().ok())
        .filter(|major| *major != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_comes_from_the_exact_character_device_entry() {
        let devices = "Character devices:\n195 nvidia\n510 nvidia-uvm-tools\n\t509\tnvidia-uvm\n10 misc\n\nBlock devices:\n8 nvidia-uvm\n8 sd\n";
        for (name, expected) in [
            ("nvidia", Some(195)),
            ("nvidia-uvm", Some(509)),
            ("nvidia-uvm-tools", Some(510)),
            ("misc", Some(10)),
            ("nvidia-uv", None),
            ("sd", None),
        ] {
            assert_eq!(character_major(devices, name), expected, "{name}");
        }
    }

    #[test]
    fn missing_or_malformed_major_is_not_guessed() {
        for devices in [
            "",
            "234 example\n",
            "Character devices:\n234 example-tools\n",
            "Character devices:\n\nBlock devices:\n234 example\n",
            "Character devices:\nbad example\n",
            "Character devices:\n-1 example\n",
            "Character devices:\n0 example\n",
            "Character devices:\n4294967296 example\n",
            "Character devices:\n234 example extra\n",
        ] {
            assert_eq!(character_major(devices, "example"), None, "{devices}");
        }
    }
}
