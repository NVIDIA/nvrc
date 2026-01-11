// SPDX-License-Identifier: Apache-2.0
// Copyright (c) NVIDIA CORPORATION

//! Unix filesystem extensions

/// File type extensions
pub trait FileTypeExt {
    fn is_fifo(&self) -> bool;
    fn is_char_device(&self) -> bool;
}

impl FileTypeExt for crate::fs::FileType {
    fn is_fifo(&self) -> bool {
        crate::fs::FileType::is_fifo(self)
    }

    fn is_char_device(&self) -> bool {
        crate::fs::FileType::is_char_device(self)
    }
}

/// Metadata extensions
pub trait MetadataExt {
    fn mode(&self) -> u32;
}

impl MetadataExt for crate::fs::Metadata {
    fn mode(&self) -> u32 {
        crate::fs::Metadata::mode(self)
    }
}
