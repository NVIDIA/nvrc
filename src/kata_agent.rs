// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use anyhow::{anyhow, Context, Result};
use rlimit::{setrlimit, Resource};
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::Command;

pub fn kata_agent() -> Result<()> {
    let nofile = 1024 * 1024; // desired RLIMIT_NOFILE
    setrlimit(Resource::NOFILE, nofile, nofile).context("setrlimit RLIMIT_NOFILE")?;
    fs::write("/proc/self/oom_score_adj", b"-997").context("write /proc/self/oom_score_adj")?;
    if let Ok(lim) = rlimit::getrlimit(Resource::NOFILE) {
        debug!("kata-agent RLIMIT_NOFILE: {:?}", lim);
    }

    let err = Command::new("/usr/bin/kata-agent").exec();
    Err(anyhow!("exec /usr/bin/kata-agent failed: {err}"))
}
