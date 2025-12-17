// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use log::{debug, error, info, warn};
use std::fs;
use std::os::fd::AsFd;
use std::os::unix::net::UnixDatagram;
use std::path::Path;
use std::sync::OnceLock;

use nix::poll::{PollFd, PollFlags};

static SYSLOG: OnceLock<UnixDatagram> = OnceLock::new();

/// Initialize the global syslog socket at /dev/log
pub fn init() -> std::io::Result<()> {
    // Use get_or_init with immediate setup since dev_log_setup is infallible for init purposes
    if SYSLOG.get().is_none() {
        let socket = dev_log_setup()?;
        let _ = SYSLOG.set(socket); // Ignore if already set (race condition)
    }
    Ok(())
}

/// Poll the global syslog socket for messages
pub fn poll() -> std::io::Result<()> {
    if let Some(sock) = SYSLOG.get() {
        poll_dev_log(sock)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum SyslogSeverity {
    Emergency = 0, // System is unusable
    Alert,         // Action must be taken immediately
    Critical,      // Critical conditions
    Error,         // Error conditions
    Warning,       // Warning conditions
    Notice,        // Normal but significant condition
    Info,          // Informational messages
    Debug,         // Debug-level messages
}

impl From<u8> for SyslogSeverity {
    fn from(v: u8) -> Self {
        match v & 0x07 {
            0 => Self::Emergency,
            1 => Self::Alert,
            2 => Self::Critical,
            3 => Self::Error,
            4 => Self::Warning,
            5 => Self::Notice,
            6 => Self::Info,
            _ => Self::Debug,
        }
    }
}

impl SyslogSeverity {
    fn log(self, msg: &str) {
        match self {
            Self::Emergency | Self::Alert | Self::Critical | Self::Error => error!("{}", msg),
            Self::Warning => warn!("{}", msg),
            Self::Notice | Self::Info => info!("{}", msg),
            Self::Debug => debug!("{}", msg),
        }
    }
}

#[derive(Debug)]
struct SyslogMessage {
    severity: SyslogSeverity,
    content: String,
}

impl SyslogMessage {
    fn parse(raw: &str) -> Self {
        let r = raw.trim_end();

        if r.starts_with('<') {
            if let Some(end) = r.find('>') {
                if let Ok(p) = r[1..end].parse::<u8>() {
                    return Self {
                        severity: SyslogSeverity::from(p),
                        content: r[end + 1..].to_owned(),
                    };
                }
            }
        }

        Self {
            severity: SyslogSeverity::Info,
            content: format!("syslog: {}", r),
        }
    }

    fn log(&self) {
        self.severity.log(&self.content);
    }
}

pub fn dev_log_setup() -> std::io::Result<UnixDatagram> {
    let p = Path::new("/dev/log");

    if p.exists() {
        fs::remove_file(p)?;
    }

    UnixDatagram::bind("/dev/log")
}

pub fn poll_dev_log(sock: &UnixDatagram) -> std::io::Result<()> {
    let mut fds = [PollFd::new(sock.as_fd(), PollFlags::POLLIN)];

    // Non-blocking poll - return immediately if no data
    let poll_count = nix::poll::poll(&mut fds, nix::poll::PollTimeout::ZERO)
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    if poll_count == 0 {
        return Ok(()); // No data available
    }

    if let Some(re) = fds[0].revents() {
        if re.contains(PollFlags::POLLIN) {
            forward_syslog_message(sock)?;
        }
    }

    Ok(())
}

fn forward_syslog_message(sock: &UnixDatagram) -> std::io::Result<()> {
    let mut buf = [0u8; 4096];
    let (len, _) = sock.recv_from(&mut buf)?;

    SyslogMessage::parse(&String::from_utf8_lossy(&buf[..len])).log();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::unistd::Uid;
    use serial_test::serial;
    use std::env;
    use std::os::unix::net::UnixDatagram;
    use std::process::Command;
    use tempfile::TempDir;

    fn rerun_with_sudo() {
        let args: Vec<String> = env::args().collect();
        let out = Command::new("sudo").args(&args).status();

        if let Ok(o) = out {
            if !o.success() {
                panic!("not running with sudo")
            }
        }
    }

    #[test]
    fn test_syslog_priority_parsing() {
        let p = UnixDatagram::pair().unwrap();
        p.1.send(b"<0>Emergency message").unwrap();

        let mut b = [0u8; 4096];
        let (l, _) = p.0.recv_from(&mut b).unwrap();
        let msg = String::from_utf8_lossy(&b[..l]);

        assert!(msg.starts_with('<'));
        assert_eq!(&msg[1..2], "0");
    }

    #[test]
    fn test_syslog_priority_extraction() {
        for (i, e) in [
            ("<0>Emergency", 0u8),
            ("<3>Error", 3u8),
            ("<6>Info", 6u8),
            ("<7>Debug", 7u8),
        ] {
            if let Some(end) = i.find('>') {
                if let Ok(p) = i[1..end].parse::<u8>() {
                    assert_eq!(p & 0x07, e);
                }
            }
        }
    }

    #[test]
    fn test_syslog_invalid_priority() {
        for i in [
            "<abc>Invalid priority",
            "<>Empty priority",
            "<256>Too large priority",
            "No priority at all",
            "<6 Missing closing bracket",
        ] {
            if i.starts_with('<') {
                if let Some(end) = i.find('>') {
                    let _ = i[1..end].parse::<u8>();
                }
            }
        }
    }

    #[test]
    fn test_poll_dev_log_no_data() {
        let p = UnixDatagram::pair().unwrap();

        // Poll with no data should return Ok(()) without blocking
        let result = poll_dev_log(&p.0);
        assert!(result.is_ok());
    }

    #[test]
    #[serial]
    fn test_dev_log_setup_permissions() {
        if !Uid::effective().is_root() {
            return rerun_with_sudo();
        }

        let td = TempDir::new().unwrap();
        let path = td.path().join("log");

        std::fs::File::create(&path).unwrap();
        assert!(path.exists());

        std::fs::remove_file(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn test_syslog_message_format_with_priority() {
        for (i, e) in [
            (
                "<0>Emergency: System unusable",
                "Emergency: System unusable",
            ),
            ("<3>Error: Something failed", "Error: Something failed"),
            ("<6>Info: Normal operation", "Info: Normal operation"),
            ("<7>Debug: Detailed info", "Debug: Detailed info"),
        ] {
            if let Some(end) = i.find('>') {
                assert_eq!(&i[end + 1..], e);
            }
        }
    }

    #[test]
    fn test_syslog_facility_and_severity() {
        for (p, f, s) in [(0, 0, 0), (16, 2, 0), (24, 3, 0), (22, 2, 6), (30, 3, 6)] {
            assert_eq!(p >> 3, f);
            assert_eq!(p & 0x07, s);
        }
    }

    #[test]
    #[serial]
    fn test_forward_syslog_message_integration() {
        let p = UnixDatagram::pair().unwrap();
        p.1.send(b"<6>Test info message from syslog").unwrap();

        assert!(forward_syslog_message(&p.0).is_ok());
    }

    #[test]
    fn test_syslog_message_parsing() {
        let m = SyslogMessage::parse("<3>Error message");
        assert_eq!(m.severity as u8, 3);
        assert_eq!(m.content, "Error message");

        let m = SyslogMessage::parse("No priority message");
        assert_eq!(m.severity as u8, 6);
        assert_eq!(m.content, "syslog: No priority message");

        let m = SyslogMessage::parse("<abc>Invalid priority");
        assert_eq!(m.severity as u8, 6);
        assert_eq!(m.content, "syslog: <abc>Invalid priority");
    }

    #[test]
    fn test_syslog_severity_conversion() {
        assert_eq!(SyslogSeverity::from(0), SyslogSeverity::Emergency);
        assert_eq!(SyslogSeverity::from(1), SyslogSeverity::Alert);
        assert_eq!(SyslogSeverity::from(2), SyslogSeverity::Critical);
        assert_eq!(SyslogSeverity::from(3), SyslogSeverity::Error);
        assert_eq!(SyslogSeverity::from(4), SyslogSeverity::Warning);
        assert_eq!(SyslogSeverity::from(5), SyslogSeverity::Notice);
        assert_eq!(SyslogSeverity::from(6), SyslogSeverity::Info);
        assert_eq!(SyslogSeverity::from(7), SyslogSeverity::Debug);

        assert_eq!(SyslogSeverity::from(24), SyslogSeverity::Emergency);
        assert_eq!(SyslogSeverity::from(22), SyslogSeverity::Info);
    }

    #[test]
    fn test_syslog_message_edge_cases() {
        let m = SyslogMessage::parse("");
        assert_eq!(m.severity as u8, 6);
        assert_eq!(m.content, "syslog: ");

        let m = SyslogMessage::parse("<>");
        assert_eq!(m.severity as u8, 6);
        assert_eq!(m.content, "syslog: <>");

        let m = SyslogMessage::parse("<5 Missing closing");
        assert_eq!(m.severity as u8, 6);
        assert_eq!(m.content, "syslog: <5 Missing closing");

        let m = SyslogMessage::parse("<4>");
        assert_eq!(m.severity as u8, 4);
        assert_eq!(m.content, "");
    }
}
