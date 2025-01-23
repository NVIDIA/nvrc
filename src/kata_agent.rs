/*
pub fn kata_agent() -> Result<()> {
    setrlimit(Resource::NOFILE, NOFILE_LIMIT, NOFILE_LIMIT).expect("Failed to set nofile limit");

    fs::write("/proc/self/oom_score_adj", b"-997").expect("Failed to write OOM score");

    let tty_file = OpenOptions::new().read(true).write(true).open("/dev/tty")?;

    // Get the original FD
    let original_fd = tty_file.as_raw_fd();
    // Duplicate it for stderr
    let duplicated_fd = dup(original_fd).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    // Convert each FD into a Stdio
    let stdout = unsafe { Stdio::from_raw_fd(original_fd) };
    let stderr = unsafe { Stdio::from_raw_fd(duplicated_fd) };

    debug!(
        "kata_agent nofile: {:?}",
        rlimit::getrlimit(Resource::NOFILE)
    );

    debug!("Starting kata-agent");
    //let cmd: = std::process::Command::new("/usr/bin/kata-agent").stdin(Stdio::null()).stdout(stdout).stderr(stderr);
    //let err = cmd.exec();

    let agent = Command::new("/usr/bin/kata-agent")
    .stdin(Stdio::null())
    .stdout(stdout)
    .stderr(stderr);

    if let Ok(mut child) = agent.spawn() {
        child.wait().expect("command wasn't running");
        debug!("Child has finished its execution!");
        Ok(())
    } else {
        debug!("ls command didn't start");
        Ok(())
    }
    //Err(anyhow!("exec of kata-agent failed: {}", err))
}
*/
/*
use std::fs;
use rlimit::{setrlimit, Resource};
use std::process::Command;
use std::os::unix::process::CommandExt;

const NOFILE_LIMIT: u64 = 1024 * 1024;

pub fn kata_agent() -> Result<(), std::io::Error> {
    setrlimit(Resource::NOFILE, NOFILE_LIMIT, NOFILE_LIMIT).expect("Failed to set nofile limit");

    fs::write("/proc/self/oom_score_adj", b"-997").expect("Failed to write OOM score");

    debug!(
        "kata_agent nofile: {:?}",
        rlimit::getrlimit(Resource::NOFILE)
    );

    let mut cmd = Command::new("/sbin/init");
    cmd.exec();
    Ok(())
}*/
use anyhow::{anyhow, Result};
use rlimit::{setrlimit, Resource};
use std::fs;
use std::fs::OpenOptions;
use std::os::fd::FromRawFd;
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::process::Stdio;

use crate::coreutils;

pub fn kata_agent() -> Result<()> {
    coreutils::cat("/proc/mounts")?;
    // 1) Open /dev/tty or another file for stdout
    let con_for_stdout = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/console")
        .expect("cannot open /dev/tty_for_stdout");

    // 2) Open /dev/tty (or dup the first) for stderr
    let con_for_stderr = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/console")
        .expect("cannot open /dev/tty_for_stderr");

    // 3) Convert them to `Stdio`. We store them in variables
    //    so they are *not* ephemeral and won't drop immediately.
    let stdout_stdio = unsafe { Stdio::from_raw_fd(con_for_stdout.as_raw_fd()) };
    let stderr_stdio = unsafe { Stdio::from_raw_fd(con_for_stderr.as_raw_fd()) };

    // 4) Now use them in a Command
    let mut child = Command::new("/usr/bin/kata-agent")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_stdio))
        .stderr(Stdio::from(stderr_stdio))
        .spawn()?;

    let status = child.wait()?;
    if status.success() {
        eprintln!("Child has finished its execution successfully!");
    } else {
        eprintln!("Child returned an error: {:?}", status.code());
    }

    Ok(())
}
