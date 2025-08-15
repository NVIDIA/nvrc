use crate::coreutils::{fs_append, Result};
use core::str;
use sc::syscall;

const NAME_LEN: usize = 32;
const O_RDONLY: i32 = 0;
const AT_FDCWD: i32 = -100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserGroup {
    pub user_id: u32,
    pub group_id: u32,
    pub user_name: [u8; NAME_LEN],
    pub group_name: [u8; NAME_LEN],
    user_name_len: usize,
    group_name_len: usize,
}

/// A helper to write a u32 to a buffer and return the written slice.
fn u32_to_str<'a>(mut n: u32, buf: &'a mut [u8; 10]) -> &'a [u8] {
    if n == 0 {
        buf[0] = b'0';
        return &buf[..1];
    }
    let mut i = buf.len();
    while n > 0 {
        i -= 1;
        buf[i] = (n % 10) as u8 + b'0';
        n /= 10;
    }
    &buf[i..]
}

impl UserGroup {
    pub fn new() -> Self {
        Self::root()
    }

    pub fn root() -> Self {
        let mut user_name = [0u8; NAME_LEN];
        user_name[..4].copy_from_slice(b"root");
        let mut group_name = [0u8; NAME_LEN];
        group_name[..4].copy_from_slice(b"root");

        Self {
            user_id: 0,
            group_id: 0,
            user_name,
            group_name,
            user_name_len: 4,
            group_name_len: 4,
        }
    }

    pub fn with_ids(uid: u32, gid: u32, user: &[u8], group: &[u8]) -> Self {
        let mut user_name = [0u8; NAME_LEN];
        user_name[..user.len()].copy_from_slice(user);
        let mut group_name = [0u8; NAME_LEN];
        group_name[..group.len()].copy_from_slice(group);

        Self {
            user_id: uid,
            group_id: gid,
            user_name,
            group_name,
            user_name_len: user.len(),
            group_name_len: group.len(),
        }
    }

    pub fn write_to_system_files(&self) -> Result<()> {
        self.write_to_files("/etc/passwd", "/etc/shadow", "/etc/group")
    }

    pub fn write_to_files(&self, pw: &str, sh: &str, gr: &str) -> Result<()> {
        let mut buf = [0u8; 256];
        fs_append(pw, self.passwd_entry(&mut buf)?)?;
        fs_append(sh, self.shadow_entry(&mut buf)?)?;
        fs_append(gr, self.group_entry(&mut buf)?)?;
        Ok(())
    }

    fn passwd_entry<'a>(&self, buf: &'a mut [u8]) -> Result<&'a [u8]> {
        let mut writer = BufferWriter::new(buf);
        writer.write(&self.user_name[..self.user_name_len]);
        writer.write(b":x:");
        writer.write_u32(self.user_id);
        writer.write(b":");
        writer.write_u32(self.group_id);
        writer.write(b":");
        writer.write(&self.user_name[..self.user_name_len]);
        writer.write(b":/nonexistent:/bin/false\n");
        Ok(writer.into_slice())
    }

    fn shadow_entry<'a>(&self, buf: &'a mut [u8]) -> Result<&'a [u8]> {
        let mut writer = BufferWriter::new(buf);
        writer.write(&self.user_name[..self.user_name_len]);
        writer.write(b":*:18295:0:99999:7:::\n");
        Ok(writer.into_slice())
    }

    fn group_entry<'a>(&self, buf: &'a mut [u8]) -> Result<&'a [u8]> {
        let mut writer = BufferWriter::new(buf);
        writer.write(&self.group_name[..self.group_name_len]);
        writer.write(b":x:");
        writer.write_u32(self.group_id);
        writer.write(b":\n");
        Ok(writer.into_slice())
    }
}

impl Default for UserGroup {
    fn default() -> Self {
        Self::new()
    }
}

/// Reads random bytes from `/dev/urandom`.
fn read_random_bytes(buf: &mut [u8]) -> Result<()> {
    let mut path_buf = [0u8; 32];
    let path_ptr = crate::coreutils::str_to_cstring("/dev/urandom", &mut path_buf)?;
    let fd = unsafe { syscall!(OPENAT, AT_FDCWD as isize, path_ptr as usize, O_RDONLY as isize) } as isize;
    if fd < 0 { return Err(crate::coreutils::CoreUtilsError::Syscall(fd)); }

    let res = unsafe { syscall!(READ, fd as usize, buf.as_mut_ptr() as usize, buf.len()) } as isize;
    let _ = unsafe { syscall!(CLOSE, fd as usize) };

    if res < 0 { return Err(crate::coreutils::CoreUtilsError::Syscall(res)); }
    Ok(())
}

pub fn random_user_group() -> Result<UserGroup> {
    let mut int_buf = [0u8; 4];
    read_random_bytes(&mut int_buf)?;
    let uid = 1000 + u32::from_ne_bytes(int_buf) % (60000 - 1000);
    read_random_bytes(&mut int_buf)?;
    let gid = 1000 + u32::from_ne_bytes(int_buf) % (60000 - 1000);

    let mut name_buf = [0u8; 8];
    read_random_bytes(&mut name_buf)?;
    for byte in &mut name_buf {
        *byte = b'a' + (*byte % 26);
    }

    let ug = UserGroup::with_ids(uid, gid, &name_buf, &name_buf);
    ug.write_to_system_files()?;
    Ok(ug)
}

/// A simple helper to write data into a byte slice buffer.
struct BufferWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> BufferWriter<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn write(&mut self, data: &[u8]) {
        let len = data.len();
        self.buf[self.pos..self.pos + len].copy_from_slice(data);
        self.pos += len;
    }

    fn write_u32(&mut self, n: u32) {
        let mut num_buf = [0u8; 10];
        let s = u32_to_str(n, &mut num_buf);
        self.write(s);
    }

    fn into_slice(self) -> &'a [u8] {
        &self.buf[..self.pos]
    }
}
