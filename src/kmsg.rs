use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader};
use std::thread::sleep;
use std::time::Duration;

pub fn kernlog_setup() {
    kernlog::init().unwrap();
    log::set_max_level(log::LevelFilter::Off);

    let kernel_buffer_size = b"16777216";

    fs::write("/proc/sys/net/core/rmem_default", kernel_buffer_size).unwrap();
    fs::write("/proc/sys/net/core/wmem_default", kernel_buffer_size).unwrap();
    fs::write("/proc/sys/net/core/rmem_max", kernel_buffer_size).unwrap();
    fs::write("/proc/sys/net/core/wmem_max", kernel_buffer_size).unwrap();
}

pub fn kmsg() -> std::fs::File {
    let log_path = if log_enabled!(log::Level::Debug) {
        "/dev/kmsg"
    } else {
        "/dev/null"
    };
    OpenOptions::new()
        .write(true)
        .open(log_path)
        .expect("failed to open /dev/kmsg")
}

pub fn watch_for_pattern(pattern: &'static str, tx: std::sync::mpsc::Sender<&'static str>) {
    let file = File::open("/dev/kmsg").expect("Could not open /dev/kmsg");
    let mut reader = BufReader::new(file);

    let mut line = String::new();
    let mut last_seq: u64 = 0; // Track the highest sequence number we've seen

    std::thread::spawn(move || loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(bytes_read) => {
                if bytes_read == 0 {
                    // No new data right now; try again soon
                    sleep(Duration::from_millis(100));
                    continue;
                }
                // Example line format: "6,1234,987654321,-;NVRM: Attempting to remove device ..."
                // We can split on commas to grab the sequence number
                let parts: Vec<&str> = line.split(',').collect();
                if parts.len() >= 3 {
                    if let Ok(seq) = parts[1].parse::<u64>() {
                        if seq <= last_seq {
                            // This line is not newer than what we've already seen; skip it.
                            continue;
                        }
                        // It's a new line, so update our last_seq
                        last_seq = seq;
                    }
                }

                // Now check for the pattern
                if line.contains(pattern) {
                    tx.send("hot-unplug").unwrap();
                }
            }
            Err(e) => {
                panic!("error reading /dev/kmsg: {e}");
            }
        }
    });
}
