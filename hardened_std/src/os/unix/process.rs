// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Unix process extensions

use crate::process::Command;

/// Command extensions for Unix
pub trait CommandExt {
    fn exec(&mut self) -> crate::Error;
}

impl CommandExt for Command {
    fn exec(&mut self) -> crate::Error {
        Command::exec(self)
    }
}
