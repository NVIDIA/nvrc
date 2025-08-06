use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader};
use std::sync::mpsc;
use std::thread::{self, sleep, JoinHandle};
use std::time::Duration;

/// Kernel buffer size for network memory settings (16MB)
const KERNEL_BUFFER_SIZE: &[u8] = b"16777216";

/// Kernel message device path
const KMSG_PATH: &str = "/dev/kmsg";

/// Null device path for disabled logging
const NULL_PATH: &str = "/dev/null";

/// Sleep duration when no new kmsg data is available
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Network core memory setting paths
const RMEM_DEFAULT_PATH: &str = "/proc/sys/net/core/rmem_default";
const WMEM_DEFAULT_PATH: &str = "/proc/sys/net/core/wmem_default";
const RMEM_MAX_PATH: &str = "/proc/sys/net/core/rmem_max";
const WMEM_MAX_PATH: &str = "/proc/sys/net/core/wmem_max";

pub fn kernlog_setup() -> Result<()> {
    kernlog::init().context("Failed to initialize kernel log")?;
    log::set_max_level(log::LevelFilter::Off);

    // Set kernel network buffer sizes for better performance
    fs::write(RMEM_DEFAULT_PATH, KERNEL_BUFFER_SIZE)
        .with_context(|| format!("Failed to write to {}", RMEM_DEFAULT_PATH))?;
    fs::write(WMEM_DEFAULT_PATH, KERNEL_BUFFER_SIZE)
        .with_context(|| format!("Failed to write to {}", WMEM_DEFAULT_PATH))?;
    fs::write(RMEM_MAX_PATH, KERNEL_BUFFER_SIZE)
        .with_context(|| format!("Failed to write to {}", RMEM_MAX_PATH))?;
    fs::write(WMEM_MAX_PATH, KERNEL_BUFFER_SIZE)
        .with_context(|| format!("Failed to write to {}", WMEM_MAX_PATH))?;

    Ok(())
}

pub fn kmsg() -> Result<File> {
    let log_path = if log_enabled!(log::Level::Debug) {
        KMSG_PATH
    } else {
        NULL_PATH
    };
    OpenOptions::new()
        .write(true)
        .open(log_path)
        .with_context(|| format!("Failed to open {}", log_path))
}

pub fn watch_for_pattern(pattern: &'static str, tx: mpsc::Sender<&'static str>) -> JoinHandle<()> {
    thread::spawn(move || {
        let file = match File::open(KMSG_PATH) {
            Ok(f) => f,
            Err(e) => {
                log::error!("Could not open {}: {}", KMSG_PATH, e);
                return;
            }
        };

        let mut reader = BufReader::new(file);
        let mut line = String::new();
        let mut last_seq: u64 = 0;

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(bytes_read) => {
                    if bytes_read == 0 {
                        // No new data right now; try again soon
                        sleep(POLL_INTERVAL);
                        continue;
                    }

                    // Parse sequence number from kmsg format: "priority,sequence,timestamp,-;message"
                    if let Some(seq) = parse_kmsg_sequence(&line) {
                        if seq <= last_seq {
                            // Skip already processed messages
                            continue;
                        }
                        last_seq = seq;
                    }

                    // Check for the pattern and send notification
                    if line.contains(pattern) {
                        if let Err(e) = tx.send("hot-unplug") {
                            log::error!("Failed to send pattern notification: {}", e);
                            break;
                        }
                    }
                }
                Err(e) => {
                    log::error!("Error reading from {}: {}", KMSG_PATH, e);
                    sleep(POLL_INTERVAL);
                }
            }
        }
    })
}

fn parse_kmsg_sequence(line: &str) -> Option<u64> {
    let parts: Vec<&str> = line.split(',').collect();
    if parts.len() >= 2 {
        parts[1].parse::<u64>().ok()
    } else {
        None
    }
}
