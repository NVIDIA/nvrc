// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use crate::kata_agent;
use crate::nvrc::NVRC;
use log::{debug, error};
use nix::unistd::{fork, ForkResult};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result};

impl NVRC {
    pub fn cold_plug(&mut self) -> Result<()> {
        debug!("cold-plug mode");
        self.setup_gpu();
        match unsafe { fork() }.expect("fork cold-plug") {
            ForkResult::Parent { .. } => {
                kata_agent().context("kata-agent cold-plug parent")?;
            }
            ForkResult::Child => loop {
                sleep(Duration::from_secs(1));
                if let Err(e) = self.poll_syslog() {
                    error!("poll syslog: {e}");
                    break;
                }
            },
        }
        Ok(())
    }
}
