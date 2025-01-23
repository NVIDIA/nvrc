use rand::Rng;
use std::fs::OpenOptions;
use std::io::Write;

#[derive(Debug, Default)]
pub struct UserGroup {
    user_id: u32,
    group_id: u32,
    pub user_name: String,
    pub group_name: String,
}

pub fn random_user_group() -> UserGroup {
    let mut rng = rand::thread_rng();
    let user_id = rng.gen_range(1000..60000); // Generating user ID in the range 1000-60000
    let group_id = rng.gen_range(1000..60000); // Generating group ID in the range 1000-60000

    let user_name: String = (0..8)
        .map(|_| (rng.gen_range(b'a'..=b'z') as char))
        .collect();
    let group_name: String = (0..8)
        .map(|_| (rng.gen_range(b'a'..=b'z') as char))
        .collect();

    let user_group = UserGroup {
        user_id,
        group_id,
        user_name,
        group_name,
    };

    add_to_passwd(&user_group);
    add_to_shadow(&user_group);
    add_to_group(&user_group);

    user_group
}

fn add_to_passwd(user_group: &UserGroup) {
    let passwd_entry = format!(
        "{}:x:{}:{}::/nonexistent:/bin/false\n",
        user_group.user_name, user_group.user_id, user_group.group_id,
    );

    let mut passwd_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/etc/passwd")
        .expect("failed to open /etc/passwd");

    passwd_file
        .write_all(passwd_entry.as_bytes())
        .expect("failed to write to /etc/passwd");
}

fn add_to_shadow(user_group: &UserGroup) {
    let shadow_entry = format!("{}:*:18295:0:99999:7:::\n", user_group.user_name);

    let mut shadow_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/etc/shadow")
        .expect("failed to open /etc/shadow");

    shadow_file
        .write_all(shadow_entry.as_bytes())
        .expect("failed to write to /etc/shadow");
}

fn add_to_group(user_group: &UserGroup) {
    let group_entry = format!("{}:x:{}:\n", user_group.group_name, user_group.group_id,);

    let mut group_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/etc/group")
        .expect("failed to open /etc/group");

    group_file
        .write_all(group_entry.as_bytes())
        .expect("failed to write to /etc/group");
}
