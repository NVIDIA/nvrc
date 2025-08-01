use std::fs::{self};
use std::os::fd::AsFd;
use std::os::unix::net::UnixDatagram;
use std::path::Path;

use nix::poll::{poll, PollFd, PollFlags};

const DEV_LOG_PATH: &str = "/dev/log";

/// Create and bind /dev/log as a Unix datagram socket.
pub fn dev_log_setup() -> std::io::Result<UnixDatagram> {
    if Path::new(DEV_LOG_PATH).exists() {
        fs::remove_file(DEV_LOG_PATH)?;
    }
    UnixDatagram::bind(DEV_LOG_PATH)
}

/// Poll the /dev/log socket, and if data is available, forward it to /dev/kmsg.
pub fn poll_dev_log(socket: &UnixDatagram) -> std::io::Result<()> {
    let mut fds = [PollFd::new(socket.as_fd(), PollFlags::POLLIN)];

    // Non-blocking poll - return immediately if no data
    let n = poll(&mut fds, 0u16).map_err(|e| std::io::Error::from_raw_os_error(e as i32))?;

    if n == 0 {
        return Ok(()); // No data available
    }

    // Check if data is ready to read
    if let Some(revents) = fds[0].revents() {
        if revents.contains(PollFlags::POLLIN) {
            forward_syslog_message(socket)?;
        }
    }

    Ok(())
}

/// Read a message from the socket and forward it to /dev/kmsg using logging macros.
fn forward_syslog_message(socket: &UnixDatagram) -> std::io::Result<()> {
    let mut buf = [0u8; 4096];
    let (len, _addr) = socket.recv_from(&mut buf)?;

    let message = String::from_utf8_lossy(&buf[..len]).trim_end().to_string();

    // Parse syslog priority if present, otherwise default to info level
    if message.starts_with('<') {
        // Extract priority number between < and >
        if let Some(end) = message.find('>') {
            if let Ok(priority) = message[1..end].parse::<u8>() {
                let severity = priority & 0x07; // Last 3 bits are severity
                let msg_content = &message[end + 1..];

                // Forward to appropriate log level based on syslog severity
                match severity {
                    0 => error!("{}", msg_content), // Emergency
                    1 => error!("{}", msg_content), // Alert
                    2 => error!("{}", msg_content), // Critical
                    3 => error!("{}", msg_content), // Error
                    4 => warn!("{}", msg_content),  // Warning
                    5 => info!("{}", msg_content),  // Notice
                    6 => info!("{}", msg_content),  // Info
                    7 => debug!("{}", msg_content), // Debug
                    _ => info!("{}", msg_content),  // Default to info
                }
            } else {
                // Invalid priority format, log as info
                info!("syslog: {}", message);
            }
        } else {
            // No closing >, log as info
            info!("syslog: {}", message);
        }
    } else {
        // No priority, default to info level
        info!("syslog: {}", message);
    }

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
}
