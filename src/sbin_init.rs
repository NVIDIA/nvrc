use rlimit::{setrlimit, Resource};
use std::os::unix::process::CommandExt;

const NOFILE_LIMIT: u64 = 1 * 1024 * 1024;

pub fn kata_agent() -> Result<(), std::io::Error> {
    setrlimit(Resource::NOFILE, NOFILE_LIMIT, NOFILE_LIMIT)
    .expect("Failed to set nofile limit");

    debug!("kata_agent nofile: {:?}", rlimit::getrlimit(Resource::NOFILE));

    let mut cmd = std::process::Command::new("/sbin/init");
    cmd.exec();
    Ok(())
}
