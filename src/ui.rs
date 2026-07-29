//! The signs the shell hooks print, kept byte-identical so output doesn't
//! change as hooks move to Rust one at a time — a user should not be able to
//! tell which implementation ran.
pub const VALID_SIGN: &str = "  \u{1b}[38;5;112m✓\u{1b}[0m";
pub const ERROR_SIGN: &str = "  \u{1b}[38;5;160m✗\u{1b}[0m";
#[allow(dead_code)]
pub const WARNING_SIGN: &str = "  \u{1b}[38;5;208m!\u{1b}[0m";
