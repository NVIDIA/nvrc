use std::env;

#[derive(Debug)]
pub enum InitInvocation {
    Init,
    SbinInit,
    Other,
}

impl InitInvocation {
    pub fn from_argv0() -> Self {
        let argv0 = env::args().next().unwrap_or_default();
        match argv0.as_str() {
            "/init" => InitInvocation::Init,
            "/sbin/init" => InitInvocation::SbinInit,
            _ => InitInvocation::Other, // or whatever makes sense for you
        }
    }
}
