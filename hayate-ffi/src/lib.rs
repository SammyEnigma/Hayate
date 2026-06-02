//! C-compatible FFI shim for hayate.
//! Reserved for future Flutter / Android / iOS integration.

#![allow(clippy::missing_safety_doc)]

/// Returns the current engine protocol version.
#[unsafe(no_mangle)]
pub extern "C" fn hayate_protocol_version() -> u16 {
    hayate::protocol::PROTOCOL_VERSION
}
