//! Unix domain sockets

use crate::{path::Path, Result};

/// Unix datagram socket
pub struct UnixDatagram {
    fd: i32,
}

impl UnixDatagram {
    pub fn bind(_path: &Path) -> Result<Self> {
        todo!("UnixDatagram::bind")
    }

    pub fn unbound() -> Result<Self> {
        todo!("UnixDatagram::unbound")
    }

    pub fn send_to(&self, _buf: &[u8], _path: &Path) -> Result<usize> {
        todo!("UnixDatagram::send_to")
    }

    pub fn recv_from(&self, _buf: &mut [u8]) -> Result<(usize, SocketAddr)> {
        todo!("UnixDatagram::recv_from")
    }
}

impl crate::os::fd::AsFd for UnixDatagram {
    fn as_fd(&self) -> i32 {
        self.fd
    }
}

/// Socket address
pub struct SocketAddr {
    _private: (),
}
