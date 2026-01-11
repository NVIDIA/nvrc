// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Process execution with static string constraints

use crate::Result;

/// Command builder
pub struct Command {
    path: &'static str,
}

impl Command {
    pub fn new(path: &'static str) -> Self {
        todo!("Command::new")
    }

    pub fn args(&mut self, args: &[&'static str]) -> &mut Self {
        todo!("Command::args")
    }

    pub fn stdout(&mut self, cfg: Stdio) -> &mut Self {
        todo!("Command::stdout")
    }

    pub fn stderr(&mut self, cfg: Stdio) -> &mut Self {
        todo!("Command::stderr")
    }

    pub fn spawn(&mut self) -> Result<Child> {
        todo!("Command::spawn")
    }

    pub fn status(&mut self) -> Result<ExitStatus> {
        todo!("Command::status")
    }

    pub fn exec(&mut self) -> crate::Error {
        todo!("Command::exec")
    }
}

/// Child process
pub struct Child {
    pid: i32,
}

impl Child {
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        todo!("Child::try_wait")
    }

    pub fn wait(&mut self) -> Result<ExitStatus> {
        todo!("Child::wait")
    }

    pub fn kill(&mut self) -> Result<()> {
        todo!("Child::kill")
    }
}

/// Exit status
pub struct ExitStatus {
    code: i32,
}

impl ExitStatus {
    pub fn success(&self) -> bool {
        todo!("ExitStatus::success")
    }
}

/// Standard I/O configuration
pub enum Stdio {
    Null,
    Inherit,
    Piped,
    Fd(i32),
}

impl Stdio {
    pub fn from(file: crate::fs::File) -> Self {
        todo!("Stdio::from")
    }
}
