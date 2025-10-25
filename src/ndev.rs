// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use std::process;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use kobject_uevent::{ActionType, UEvent};
use log::{debug, trace};
use netlink_sys::{protocols::NETLINK_KOBJECT_UEVENT, Socket, SocketAddr};

fn is_nvidia_gpu(e: &UEvent) -> bool {
    match (e.env.get("PCI_ID"), e.env.get("PCI_CLASS")) {
        (Some(id), Some(class)) => {
            if let Some(vendor) = id.split(':').next() {
                vendor == "10DE" && (class == "30200" || class == "30000")
            } else {
                false
            }
        }
        _ => false,
    }
}

pub fn udev(tx: mpsc::Sender<&'static str>) -> JoinHandle<()> {
    debug!("udev monitor start");

    // Setup netlink socket for kernel uevents
    let mut socket = Socket::new(NETLINK_KOBJECT_UEVENT).expect("netlink socket");
    socket
        .bind(&SocketAddr::new(process::id(), 1))
        .expect("bind netlink");

    thread::spawn(move || {
        loop {
            // Receive netlink packet
            let packet = match socket.recv_from_full() {
                Ok(p) => p,
                Err(e) => {
                    log::error!("recv netlink: {e}");
                    continue;
                }
            };

            // Parse UEvent from packet
            let uevent = match UEvent::from_netlink_packet(&packet.0) {
                Ok(u) => u,
                Err(e) => {
                    log::error!("parse uevent: {e}");
                    continue;
                }
            };

            if let Ok(raw) = std::str::from_utf8(&packet.0) {
                trace!("raw uevent: {raw}");
            }
            trace!("uevent: {:?}", uevent);

            // Check for NVIDIA GPU add events
            if uevent.action == ActionType::Add && is_nvidia_gpu(&uevent) {
                debug!("gpu add detected");
                thread::sleep(Duration::from_secs(5));
                if let Err(e) = tx.send("hot-plug") {
                    error!("send hot-plug: {e}");
                    break;
                }
            }
        }
    })
}
