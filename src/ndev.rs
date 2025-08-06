use std::process;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kobject_uevent::{ActionType, UEvent};
use log::{debug, trace};
use netlink_sys::{protocols::NETLINK_KOBJECT_UEVENT, Socket, SocketAddr};

const NVIDIA_VENDOR_ID: &str = "10DE";
const PCI_CLASS_3D: &str = "30200";
const PCI_CLASS_DISPLAY: &str = "30000";

const DEFAULT_HOTPLUG_TIMEOUT: u64 = 5;
const UDEV_GROUP_ID: u32 = 1;

fn is_nvidia_gpu(event: &UEvent) -> bool {
    let pci_id = match event.env.get("PCI_ID") {
        Some(id) => id,
        None => return false,
    };

    let pci_class = match event.env.get("PCI_CLASS") {
        Some(class) => class,
        None => return false,
    };

    // Parse vendor ID from PCI_ID (format: "vendor:device")
    let vendor_id = match pci_id.split(':').next() {
        Some(vendor) => vendor,
        None => return false,
    };

    vendor_id == NVIDIA_VENDOR_ID && (pci_class == PCI_CLASS_3D || pci_class == PCI_CLASS_DISPLAY)
}

fn get_current_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time is before UNIX epoch")
        .as_secs()
}

fn hotplug_device(timeout: u64) -> bool {
    let mut last_gpu_plug_timestamp = get_current_time();
    loop {
        thread::sleep(Duration::from_secs(timeout));

        if check_hotplug_activity(&mut last_gpu_plug_timestamp, timeout) {
            return true;
        }
    }
}

/// Checks if enough time has passed since the last hotplug activity
fn check_hotplug_activity(last_timestamp: &mut u64, wait_time: u64) -> bool {
    let current_time = get_current_time();
    let time_diff = current_time - *last_timestamp;

    *last_timestamp = current_time;

    time_diff >= wait_time
}

pub fn udev(tx: mpsc::Sender<&'static str>) -> JoinHandle<()> {
    debug!("Starting udev monitoring for NVIDIA GPU events");

    // Setup netlink socket for kernel uevents
    let mut socket = Socket::new(NETLINK_KOBJECT_UEVENT).expect("Failed to create netlink socket");
    let sa = SocketAddr::new(process::id(), UDEV_GROUP_ID);
    socket.bind(&sa).expect("Failed to bind netlink socket");

    thread::spawn(move || {
        loop {
            // Receive netlink packet
            let packet = match socket.recv_from_full() {
                Ok(packet) => packet,
                Err(e) => {
                    log::error!("Failed to receive netlink packet: {}", e);
                    continue;
                }
            };

            // Parse UEvent from packet
            let uevent = match UEvent::from_netlink_packet(&packet.0) {
                Ok(event) => event,
                Err(e) => {
                    log::error!("Failed to parse UEvent: {}", e);
                    continue;
                }
            };

            // Log the raw packet for debugging
            if let Ok(raw_str) = std::str::from_utf8(&packet.0) {
                debug!("UEvent: {}", raw_str);
            }
            trace!("Parsed UEvent: {:?}", uevent);

            // Check for NVIDIA GPU add events
            if uevent.action == ActionType::Add && is_nvidia_gpu(&uevent) {
                debug!("NVIDIA GPU add event detected, waiting for hotplug to settle");

                if hotplug_device(DEFAULT_HOTPLUG_TIMEOUT) {
                    debug!("Hotplug activity settled, sending notification");
                    if let Err(e) = tx.send("hot-plug") {
                        log::error!("Failed to send hotplug notification: {}", e);
                        break;
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn create_test_uevent(pci_id: &str, pci_class: &str) -> UEvent {
        let mut env = HashMap::new();
        env.insert("PCI_ID".to_string(), pci_id.to_string());
        env.insert("PCI_CLASS".to_string(), pci_class.to_string());

        UEvent {
            action: ActionType::Add,
            devpath: PathBuf::from("/test"),
            subsystem: "pci".to_string(),
            seq: 0, // Sequence number for tests
            env,
        }
    }

    #[test]
    fn test_is_nvidia_gpu_valid_3d() {
        let event = create_test_uevent("10DE:1234", PCI_CLASS_3D);
        assert!(is_nvidia_gpu(&event));
    }

    #[test]
    fn test_is_nvidia_gpu_valid_display() {
        let event = create_test_uevent("10DE:5678", PCI_CLASS_DISPLAY);
        assert!(is_nvidia_gpu(&event));
    }

    #[test]
    fn test_is_nvidia_gpu_wrong_vendor() {
        let event = create_test_uevent("8086:1234", PCI_CLASS_3D);
        assert!(!is_nvidia_gpu(&event));
    }

    #[test]
    fn test_is_nvidia_gpu_wrong_class() {
        let event = create_test_uevent("10DE:1234", "40000");
        assert!(!is_nvidia_gpu(&event));
    }

    #[test]
    fn test_is_nvidia_gpu_missing_pci_id() {
        let mut env = HashMap::new();
        env.insert("PCI_CLASS".to_string(), PCI_CLASS_3D.to_string());

        let event = UEvent {
            action: ActionType::Add,
            devpath: PathBuf::from("/test"),
            subsystem: "pci".to_string(),
            seq: 0,
            env,
        };

        assert!(!is_nvidia_gpu(&event));
    }

    #[test]
    fn test_is_nvidia_gpu_missing_pci_class() {
        let mut env = HashMap::new();
        env.insert("PCI_ID".to_string(), "10DE:1234".to_string());

        let event = UEvent {
            action: ActionType::Add,
            devpath: PathBuf::from("/test"),
            subsystem: "pci".to_string(),
            seq: 0,
            env,
        };

        assert!(!is_nvidia_gpu(&event));
    }

    #[test]
    fn test_constants() {
        assert_eq!(NVIDIA_VENDOR_ID, "10DE");
        assert_eq!(PCI_CLASS_3D, "30200");
        assert_eq!(PCI_CLASS_DISPLAY, "30000");
        assert_eq!(DEFAULT_HOTPLUG_TIMEOUT, 5);
        assert_eq!(UDEV_GROUP_ID, 1);
    }
}
