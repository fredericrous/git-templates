//! One module per ported hook. Phase 1 of docs/rust-migration.md moves them
//! across one at a time; each keeps a shim at its original path so the existing
//! .zsh suite exercises the Rust implementation unchanged.
pub mod ban_terms;
pub mod branch_pattern;
pub mod commit_msg;
pub mod prepare_commit_msg;
pub mod pull_rebase;
pub mod usual_name;
