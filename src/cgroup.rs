use anyhow::Result;
use std::fs::{read_to_string, OpenOptions};
use std::io::Write;

pub fn set_cgroup_subtree_control() -> Result<()> {
    let path = "/sys/fs/cgroup/cgroup.subtree_control";
    let mut file = OpenOptions::new().write(true).open(path)?;

    file.write_all(b"+cpuset +cpu +io +memory +hugetlb +pids +rdma +misc\n")
        .unwrap();

    let contents = read_to_string(path)?;
    debug!("successfully wrote {:?} new values to {:?}", contents, path);

    Ok(())
}
