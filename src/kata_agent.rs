use anyhow::{anyhow, Context, Result};
use rlimit::{setrlimit, Resource};
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::Command;

const NOFILE_LIMIT: u64 = 1024 * 1024;
const KATA_AGENT_PATH: &str = "/usr/bin/kata-agent";
const OOM_SCORE_ADJ_PATH: &str = "/proc/self/oom_score_adj";
const OOM_SCORE_VALUE: &[u8] = b"-997";

pub fn kata_agent() -> Result<()> {
    setrlimit(Resource::NOFILE, NOFILE_LIMIT, NOFILE_LIMIT)
        .with_context(|| format!("Failed to set NOFILE limit to {}", NOFILE_LIMIT))?;

    fs::write(OOM_SCORE_ADJ_PATH, OOM_SCORE_VALUE).with_context(|| {
        format!(
            "Failed to write OOM score adjustment to {}",
            OOM_SCORE_ADJ_PATH
        )
    })?;

    if let Ok(limit) = rlimit::getrlimit(Resource::NOFILE) {
        log::debug!("kata_agent nofile limit set to: {:?}", limit);
    }

    let exec_error = Command::new(KATA_AGENT_PATH).exec();
    Err(anyhow!(
        "Failed to exec {}: {}",
        KATA_AGENT_PATH,
        exec_error
    ))
}
