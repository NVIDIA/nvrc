// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Network interface setup for the minimal init environment.
//!
//! The kernel creates the loopback device but does not bring it up—that is
//! the responsibility of the init process (PID 1). Services like
//! nv-fabricmanager bind to 127.0.0.1 and fail with ENETUNREACH if `lo`
//! is down.

use crate::macros::ResultExt;
use nix::errno::Errno;

const LOOPBACK: [libc::c_char; 3] = [b'l' as libc::c_char, b'o' as libc::c_char, 0];

nix::ioctl_read_bad!(siocgifflags, libc::SIOCGIFFLAGS, libc::ifreq);
nix::ioctl_write_ptr_bad!(siocsifflags, libc::SIOCSIFFLAGS, libc::ifreq);

/// Bring up the loopback interface (`lo`).
pub fn loopback_up() {
    let fd = Errno::result(unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0) })
        .or_panic(format_args!("socket for loopback ioctl"));

    let mut ifr: libc::ifreq = unsafe { std::mem::zeroed() };
    ifr.ifr_name[..LOOPBACK.len()].copy_from_slice(&LOOPBACK);

    unsafe {
        siocgifflags(fd, &mut ifr).or_panic(format_args!("SIOCGIFFLAGS lo"));
        ifr.ifr_ifru.ifru_flags |= (libc::IFF_UP | libc::IFF_RUNNING) as i16;
        siocsifflags(fd, &ifr).or_panic(format_args!("SIOCSIFFLAGS lo"));
    }

    let _ = unsafe { libc::close(fd) };
    info!("loopback interface up");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::require_root;
    use std::panic;

    #[test]
    fn test_loopback_up() {
        require_root();
        // lo is already up on a normal system, calling again is idempotent
        loopback_up();
    }

    #[test]
    fn test_loopback_up_idempotent() {
        require_root();
        loopback_up();
        loopback_up();
    }

    #[test]
    fn test_loopback_up_verifies_flags() {
        require_root();
        loopback_up();

        let fd = Errno::result(unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0) })
            .expect("socket");

        let mut ifr: libc::ifreq = unsafe { std::mem::zeroed() };
        ifr.ifr_name[..LOOPBACK.len()].copy_from_slice(&LOOPBACK);

        unsafe {
            siocgifflags(fd, &mut ifr).expect("SIOCGIFFLAGS");
            let flags = ifr.ifr_ifru.ifru_flags as i32;
            assert!(flags & libc::IFF_UP != 0, "IFF_UP not set");
            assert!(flags & libc::IFF_RUNNING != 0, "IFF_RUNNING not set");
            assert!(flags & libc::IFF_LOOPBACK != 0, "IFF_LOOPBACK not set");
        }

        let _ = unsafe { libc::close(fd) };
    }

    #[test]
    fn test_loopback_const() {
        assert_eq!(LOOPBACK, [b'l' as libc::c_char, b'o' as libc::c_char, 0]);
        assert_eq!(LOOPBACK.len(), 3);
    }

    #[test]
    fn test_siocgifflags_invalid_interface() {
        require_root();

        let fd = Errno::result(unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0) })
            .expect("socket");

        let mut ifr: libc::ifreq = unsafe { std::mem::zeroed() };
        let bad: [libc::c_char; 3] = [b'x' as libc::c_char, b'x' as libc::c_char, 0];
        ifr.ifr_name[..3].copy_from_slice(&bad);

        let result = unsafe { siocgifflags(fd, &mut ifr) };
        assert!(
            result.is_err(),
            "SIOCGIFFLAGS should fail for nonexistent interface"
        );

        let _ = unsafe { libc::close(fd) };
    }
}
