use std::process;

use netlink_sys::{protocols::NETLINK_KOBJECT_UEVENT, Socket, SocketAddr};

use kobject_uevent::ActionType;
use kobject_uevent::UEvent;

use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const NVIDIA_VENDOR_ID: &str = "10DE";
const PCI_CLASS_3D: &str = "30200";
const PCI_CLASS_DISPLAY: &str = "30000";

fn is_nvidia_gpu(event: &UEvent) -> bool {
    if !event.env.contains_key("PCI_ID") {
        return false;
    }
    let pci_id = event.env["PCI_ID"].split(':').collect::<Vec<&str>>();
    let pci_class = &event.env["PCI_CLASS"];

    pci_id[0] == NVIDIA_VENDOR_ID && (pci_class == PCI_CLASS_3D || pci_class == PCI_CLASS_DISPLAY)
}

// Function to get the current time in seconds since the UNIX epoch
fn get_current_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
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

fn check_hotplug_activity(last_timestamp: &mut u64, wait_time: u64) -> bool {
    let current_time = get_current_time();
    let time_diff = current_time - *last_timestamp;

    *last_timestamp = current_time;

    time_diff >= wait_time
}

pub fn udev() {
    debug!("starting udev");

    let mut socket = Socket::new(NETLINK_KOBJECT_UEVENT).unwrap();
    let sa = SocketAddr::new(process::id(), 1);
    socket.bind(&sa).unwrap();

    loop {
        let n = socket.recv_from_full().unwrap();
        let uevent = UEvent::from_netlink_packet(&n.0).unwrap();

        debug!(">> {}", std::str::from_utf8(&n.0).unwrap());
        trace!("{:#?}", uevent);

        if uevent.action == ActionType::Add && is_nvidia_gpu(&uevent) && hotplug_device(5) {
            info!("hotplug activity finished, proceeding.");
            break;
        }
    }
}
