// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{anyhow, Context, Result};
use log::{debug, error};
use nix::unistd::{fork, ForkResult};
use rlimit::{setrlimit, Resource};
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

fn kata_agent() -> Result<()> {
    let nofile = 1024 * 1024; // desired RLIMIT_NOFILE
    setrlimit(Resource::NOFILE, nofile, nofile).context("setrlimit RLIMIT_NOFILE")?;
    fs::write("/proc/self/oom_score_adj", b"-997").context("write /proc/self/oom_score_adj")?;
    let lim = rlimit::getrlimit(Resource::NOFILE)?;

    debug!("kata-agent RLIMIT_NOFILE: {:?}", lim);

    let err = Command::new("/usr/bin/kata-agent").exec();
    Err(anyhow!("exec /usr/bin/kata-agent failed: {err}"))
}

pub fn fork_agent() -> Result<()> {
    match unsafe { fork() }.expect("fork agent") {
        ForkResult::Parent { .. } => {
            kata_agent().context("kata-agent parent")?;
        }
        ForkResult::Child => loop {
            sleep(Duration::from_millis(500));
            if let Err(e) = crate::syslog::poll() {
                error!("poll syslog: {e}");
                break;
            }
        },
    }
    Ok(())
}
