// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

use std::env;

#[derive(Debug)]
pub enum Invocation {
    Init,
    SbinInit,
    Other,
}

impl Invocation {
    /// Determines the invocation type based on argv[0]
    pub fn from_argv0() -> Self {
        let argv0 = env::args().next().unwrap_or_default();
        match argv0.as_str() {
            "/init" => Self::Init,
            "/sbin/init" => Self::SbinInit,
            _ => Self::Other,
        }
    }
}
