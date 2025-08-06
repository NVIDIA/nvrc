use std::env;

const INIT_PATH: &str = "/init";
const SBIN_INIT_PATH: &str = "/sbin/init";

#[derive(Debug)]
pub enum InitInvocation {
    Init,
    SbinInit,
    Other,
}

impl InitInvocation {
    /// Determines the invocation type based on argv[0]
    pub fn from_argv0() -> Self {
        let argv0 = env::args().next().unwrap_or_default();
        match argv0.as_str() {
            INIT_PATH => InitInvocation::Init,
            SBIN_INIT_PATH => InitInvocation::SbinInit,
            _ => InitInvocation::Other,
        }
    }
}
