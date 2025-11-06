// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader};
use std::sync::mpsc;
use std::thread::{self, sleep, JoinHandle};
use std::time::Duration;

// Keep only poll interval constant (readability)
const POLL_INTERVAL: Duration = Duration::from_millis(100);

pub fn kernlog_setup() -> Result<()> {
    kernlog::init().context("kernel log init")?;
    log::set_max_level(log::LevelFilter::Off);
    // Write large buffer size to related kernel params
    for path in [
        "/proc/sys/net/core/rmem_default",
        "/proc/sys/net/core/wmem_default",
        "/proc/sys/net/core/rmem_max",
        "/proc/sys/net/core/wmem_max",
    ] {
        fs::write(path, b"16777216").with_context(|| format!("write {}", path))?;
    }
    Ok(())
}

pub fn kmsg() -> Result<File> {
    let path = if log_enabled!(log::Level::Debug) {
        "/dev/kmsg"
    } else {
        "/dev/null"
    };
    OpenOptions::new()
        .write(true)
        .open(path)
        .with_context(|| format!("open {}", path))
}

pub fn watch_for_pattern(
    pattern: &'static str,
    tx: mpsc::SyncSender<&'static str>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let file = match File::open("/dev/kmsg") {
            Ok(f) => f,
            Err(e) => {
                log::error!("open /dev/kmsg: {}", e);
                return;
            }
        };
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        let mut last_seq = 0u64;
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    sleep(POLL_INTERVAL);
                    continue;
                }
                Ok(_) => {
                    if let Some(seq) = parse_kmsg_sequence(&line) {
                        if seq <= last_seq {
                            continue;
                        }
                        last_seq = seq;
                    }
                    if line.contains(pattern) && tx.send("hot-unplug").is_err() {
                        log::error!("send pattern notification failed");
                        break;
                    }
                }
                Err(e) => {
                    log::error!("read /dev/kmsg: {}", e);
                    sleep(POLL_INTERVAL);
                }
            }
        }
    })
}

fn parse_kmsg_sequence(line: &str) -> Option<u64> {
    line.split(',').nth(1)?.parse().ok()
}
