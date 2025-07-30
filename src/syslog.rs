use std::fs::{self};
use std::os::unix::net::UnixDatagram;
use std::os::fd::AsFd;
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
                    0 => error!("{}", msg_content),    // Emergency
                    1 => error!("{}", msg_content),    // Alert
                    2 => error!("{}", msg_content),    // Critical
                    3 => error!("{}", msg_content),    // Error
                    4 => warn!("{}", msg_content),     // Warning
                    5 => info!("{}", msg_content),     // Notice
                    6 => info!("{}", msg_content),     // Info
                    7 => debug!("{}", msg_content),    // Debug
                    _ => info!("{}", msg_content),     // Default to info
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
