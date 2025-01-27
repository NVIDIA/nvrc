use anyhow::{anyhow, Result};
use rlimit::{setrlimit, Resource};
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::Command;

const NOFILE_LIMIT: u64 = 1024 * 1024;

pub fn kata_agent() -> Result<()> {
    setrlimit(Resource::NOFILE, NOFILE_LIMIT, NOFILE_LIMIT).expect("Failed to set nofile limit");
    fs::write("/proc/self/oom_score_adj", b"-997").expect("Failed to write OOM score");

    debug!(
        "kata_agent nofile: {:?}",
        rlimit::getrlimit(Resource::NOFILE)
    );
    let exec_error = Command::new("/usr/bin/kata-agent").exec();
    Err(anyhow!("exec of kata-agent failed: {}", exec_error))
}
