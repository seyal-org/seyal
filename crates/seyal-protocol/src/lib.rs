//! Versioned Seyal local protocol and disposable display projection values.
//!
//! This crate is authority-neutral: it owns wire/value validation only. Runtime
//! remains the execution/attachment authority and clients own only disposable
//! decoded display state.

pub use seyal_core::{AttachmentId, BlockId, ExecutionId};

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub mod discovery;
pub mod display;
pub mod framing;
mod pass7;
pub mod pass8;
pub mod runtime_dir;

/// Compatibility namespace matching the protocol modules' historical Runtime
/// location while Runtime and clients migrate to the physical protocol crate.
/// This is a module alias only; it contains no Runtime implementation.
pub mod local_ipc {
    #[cfg(target_os = "macos")]
    pub mod discovery {
        pub use crate::discovery::*;
    }

    pub mod framing {
        pub use crate::framing::*;
    }
}
