//! OS-specific functionality

pub mod unix {
    pub mod net;
    pub mod fs;
    pub mod process;
}

pub mod fd {
    pub trait AsFd {
        fn as_fd(&self) -> i32;
    }
}
