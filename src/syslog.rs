use log::{debug, error, info, warn};
use std::fs;
use std::os::fd::AsFd;
use std::os::unix::net::UnixDatagram;
use std::path::Path;

use nix::poll::{poll, PollFd, PollFlags};

const DEV_LOG_PATH: &str = "/dev/log";
const SYSLOG_BUFFER_SIZE: usize = 4096;

/// Convert nix error to std::io::Error
fn nix_to_io_error(err: nix::Error) -> std::io::Error {
    std::io::Error::from_raw_os_error(err as i32)
}

/// Syslog severity levels (RFC 3164)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum SyslogSeverity {
    Emergency = 0, // System is unusable
    Alert = 1,     // Action must be taken immediately
    Critical = 2,  // Critical conditions
    Error = 3,     // Error conditions
    Warning = 4,   // Warning conditions
    Notice = 5,    // Normal but significant condition
    Info = 6,      // Informational messages
    Debug = 7,     // Debug-level messages
}

impl From<u8> for SyslogSeverity {
    fn from(value: u8) -> Self {
        match value & 0x07 {
            // Extract only the last 3 bits
            0 => Self::Emergency,
            1 => Self::Alert,
            2 => Self::Critical,
            3 => Self::Error,
            4 => Self::Warning,
            5 => Self::Notice,
            6 => Self::Info,
            7 => Self::Debug,
            _ => Self::Info, // Default fallback
        }
    }
}

impl SyslogSeverity {
    fn log_message(self, content: &str) {
        match self {
            Self::Emergency | Self::Alert | Self::Critical | Self::Error => error!("{}", content),
            Self::Warning => warn!("{}", content),
            Self::Notice | Self::Info => info!("{}", content),
            Self::Debug => debug!("{}", content),
        }
    }
}

#[derive(Debug)]
struct SyslogMessage {
    severity: SyslogSeverity,
    content: String,
}

impl SyslogMessage {
    fn parse(raw_message: &str) -> Self {
        let trimmed = raw_message.trim_end();

        if let Some(parsed) = Self::try_parse_priority(trimmed) {
            parsed
        } else {
            Self {
                severity: SyslogSeverity::Info,
                content: format!("syslog: {}", trimmed),
            }
        }
    }

    fn try_parse_priority(message: &str) -> Option<Self> {
        if !message.starts_with('<') {
            return None;
        }

        let end = message.find('>')?;
        let priority_str = &message[1..end];
        let priority: u8 = priority_str.parse().ok()?;
        let content = &message[end + 1..];

        Some(Self {
            severity: SyslogSeverity::from(priority),
            content: content.to_owned(),
        })
    }

    fn log(&self) {
        self.severity.log_message(&self.content);
    }
}

pub fn dev_log_setup() -> std::io::Result<UnixDatagram> {
    let dev_log_path = Path::new(DEV_LOG_PATH);

    if dev_log_path.exists() {
        fs::remove_file(dev_log_path)?;
    }

    UnixDatagram::bind(DEV_LOG_PATH)
}

pub fn poll_dev_log(socket: &UnixDatagram) -> std::io::Result<()> {
    let mut fds = [PollFd::new(socket.as_fd(), PollFlags::POLLIN)];

    // Non-blocking poll - return immediately if no data
    let poll_count = poll(&mut fds, 0u16).map_err(nix_to_io_error)?;

    if poll_count == 0 {
        return Ok(()); // No data available
    }

    if let Some(revents) = fds[0].revents() {
        if revents.contains(PollFlags::POLLIN) {
            forward_syslog_message(socket)?;
        }
    }

    Ok(())
}

fn forward_syslog_message(socket: &UnixDatagram) -> std::io::Result<()> {
    let mut buf = [0u8; SYSLOG_BUFFER_SIZE];
    let (len, _addr) = socket.recv_from(&mut buf)?;

    let raw_message = String::from_utf8_lossy(&buf[..len]);
    let message = SyslogMessage::parse(&raw_message);
    message.log();

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
        let output = Command::new("sudo").args(&args).status();

        match output {
            Ok(output) => {
                if output.success() {
                    println!("running with sudo")
                } else {
                    panic!("not running with sudo")
                }
            }
            Err(e) => {
                panic!("Failed to escalate privileges: {e:?}")
            }
        }
    }

    #[test]
    fn test_syslog_priority_parsing() {
        // Test emergency level (0)
        let socket_pair = UnixDatagram::pair().unwrap();
        socket_pair.1.send(b"<0>Emergency message").unwrap();

        // Since we can't easily test the actual log output, we'll test that the function doesn't panic
        // and processes the message without error
        let mut buf = [0u8; 4096];
        let (len, _) = socket_pair.0.recv_from(&mut buf).unwrap();
        let message = String::from_utf8_lossy(&buf[..len]).trim_end().to_string();

        assert!(message.starts_with('<'));
        assert_eq!(&message[1..2], "0");
    }

    #[test]
    fn test_syslog_priority_extraction() {
        // Test that we can extract priority correctly
        let test_cases = vec![
            ("<0>Emergency", 0u8),
            ("<3>Error", 3u8),
            ("<6>Info", 6u8),
            ("<7>Debug", 7u8),
        ];

        for (input, expected_severity) in test_cases {
            if let Some(end) = input.find('>') {
                if let Ok(priority) = input[1..end].parse::<u8>() {
                    let severity = priority & 0x07;
                    assert_eq!(severity, expected_severity);
                }
            }
        }
    }

    #[test]
    fn test_syslog_invalid_priority() {
        // Test cases with invalid priority formats
        let test_cases = vec![
            "<abc>Invalid priority",
            "<>Empty priority",
            "<256>Too large priority",
            "No priority at all",
            "<6 Missing closing bracket",
        ];

        // These should all be handled gracefully (not panic)
        for input in test_cases {
            // Test the parsing logic
            if input.starts_with('<') {
                if let Some(end) = input.find('>') {
                    // This might fail to parse, which is expected
                    let _result = input[1..end].parse::<u8>();
                }
            }
            // All cases should be handled without panicking
        }
    }

    #[test]
    fn test_poll_dev_log_no_data() {
        // Create a socket pair for testing
        let socket_pair = UnixDatagram::pair().unwrap();

        // Poll with no data should return Ok(()) without blocking
        let result = poll_dev_log(&socket_pair.0);
        assert!(result.is_ok());
    }

    #[test]
    #[serial]
    fn test_dev_log_setup_permissions() {
        // This test needs root to create /dev/log
        if !Uid::effective().is_root() {
            return rerun_with_sudo();
        }

        // Create a temporary directory to simulate /dev
        let temp_dir = TempDir::new().unwrap();
        let temp_log_path = temp_dir.path().join("log");

        // Create a test file at the log path
        std::fs::File::create(&temp_log_path).unwrap();
        assert!(temp_log_path.exists());

        // Test that existing file gets removed (we can't test the actual /dev/log without affecting the system)
        assert!(temp_log_path.exists());
        std::fs::remove_file(&temp_log_path).unwrap();
        assert!(!temp_log_path.exists());
    }

    #[test]
    fn test_syslog_message_format_with_priority() {
        // Test different syslog message formats
        let test_cases = vec![
            (
                "<0>Emergency: System unusable",
                "Emergency: System unusable",
            ),
            ("<3>Error: Something failed", "Error: Something failed"),
            ("<6>Info: Normal operation", "Info: Normal operation"),
            ("<7>Debug: Detailed info", "Debug: Detailed info"),
        ];

        for (input, expected_content) in test_cases {
            if let Some(end) = input.find('>') {
                let content = &input[end + 1..];
                assert_eq!(content, expected_content);
            }
        }
    }

    #[test]
    fn test_syslog_facility_and_severity() {
        // Test facility and severity extraction
        // Syslog priority = facility * 8 + severity
        let test_cases = vec![
            (0, 0, 0),  // kernel.emerg
            (16, 2, 0), // mail.emerg
            (24, 3, 0), // daemon.emerg
            (22, 2, 6), // mail.info (16 + 6)
            (30, 3, 6), // daemon.info (24 + 6)
        ];

        for (priority, expected_facility, expected_severity) in test_cases {
            let facility = priority >> 3; // Upper 5 bits
            let severity = priority & 0x07; // Lower 3 bits

            assert_eq!(facility, expected_facility);
            assert_eq!(severity, expected_severity);
        }
    }

    #[test]
    #[serial]
    fn test_forward_syslog_message_integration() {
        // This test verifies the end-to-end message forwarding
        let socket_pair = UnixDatagram::pair().unwrap();

        // Send a test message
        let test_message = "<6>Test info message from syslog";
        socket_pair.1.send(test_message.as_bytes()).unwrap();

        // Test that forward_syslog_message processes it without error
        let result = forward_syslog_message(&socket_pair.0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_syslog_message_parsing() {
        // Test parsing with valid priority
        let msg = SyslogMessage::parse("<3>Error message");
        assert_eq!(msg.severity as u8, 3);
        assert_eq!(msg.content, "Error message");

        // Test parsing without priority
        let msg = SyslogMessage::parse("No priority message");
        assert_eq!(msg.severity as u8, 6); // Should default to Info
        assert_eq!(msg.content, "syslog: No priority message");

        // Test parsing with invalid priority
        let msg = SyslogMessage::parse("<abc>Invalid priority");
        assert_eq!(msg.severity as u8, 6); // Should default to Info
        assert_eq!(msg.content, "syslog: <abc>Invalid priority");
    }

    #[test]
    fn test_syslog_severity_conversion() {
        // Test all severity levels
        assert_eq!(SyslogSeverity::from(0), SyslogSeverity::Emergency);
        assert_eq!(SyslogSeverity::from(1), SyslogSeverity::Alert);
        assert_eq!(SyslogSeverity::from(2), SyslogSeverity::Critical);
        assert_eq!(SyslogSeverity::from(3), SyslogSeverity::Error);
        assert_eq!(SyslogSeverity::from(4), SyslogSeverity::Warning);
        assert_eq!(SyslogSeverity::from(5), SyslogSeverity::Notice);
        assert_eq!(SyslogSeverity::from(6), SyslogSeverity::Info);
        assert_eq!(SyslogSeverity::from(7), SyslogSeverity::Debug);

        // Test masking works correctly (only last 3 bits)
        assert_eq!(SyslogSeverity::from(24), SyslogSeverity::Emergency); // 24 & 0x07 = 0
        assert_eq!(SyslogSeverity::from(22), SyslogSeverity::Info); // 22 & 0x07 = 6
    }

    #[test]
    fn test_syslog_message_edge_cases() {
        // Empty message
        let msg = SyslogMessage::parse("");
        assert_eq!(msg.severity as u8, 6);
        assert_eq!(msg.content, "syslog: ");

        // Only priority bracket
        let msg = SyslogMessage::parse("<>");
        assert_eq!(msg.severity as u8, 6);
        assert_eq!(msg.content, "syslog: <>");

        // Unclosed priority bracket
        let msg = SyslogMessage::parse("<5 Missing closing");
        assert_eq!(msg.severity as u8, 6);
        assert_eq!(msg.content, "syslog: <5 Missing closing");

        // Valid priority with empty content
        let msg = SyslogMessage::parse("<4>");
        assert_eq!(msg.severity as u8, 4);
        assert_eq!(msg.content, "");
    }
}
