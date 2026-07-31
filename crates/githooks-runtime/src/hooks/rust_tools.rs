//! The three Rust hooks: `cargo fmt --check`, `cargo clippy`, `cargo test`.
//!
//! Scoped like every other language hook — they fire only when the commit (or
//! push) touches Rust, and only in a directory that actually has a `Cargo.toml`.
//! A Python repo never invokes cargo.
//!
//! Split across the two dispatchers by COST, matching what the other languages
//! already do: `fmt` and `clippy` are pre-commit (as ruff and pyright are),
//! `test` is pre-push (as `run-tests-js` is). Nobody wants to wait for a
//! workspace test run on every commit.
//!
//! Each is a separate check rather than one "rust" hook, so `hook.skip` can
//! disable them individually — `git config hook.skip clippy` when you are
//! mid-refactor, without losing the formatting gate.

use super::common::{fail, hl, ok, repo_root, run as run_tool, staged_files, warn, which};
use crate::check::Outcome;
use crate::git;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Files that mean "this commit touches Rust". `Cargo.toml` and `Cargo.lock`
/// count: a dependency bump compiles differently without a single `.rs` edit,
/// and that is exactly when clippy earns its keep.
const RUST_PATHS: [&str; 5] = [
    ".rs",
    "Cargo.toml",
    "Cargo.lock",
    "rustfmt.toml",
    "clippy.toml",
];

fn is_rust_path(f: &str) -> bool {
    let name = f.rsplit('/').next().unwrap_or(f);
    RUST_PATHS.iter().any(|pattern| {
        if pattern.starts_with('.') {
            name.ends_with(pattern)
        } else {
            name == *pattern
        }
    })
}

/// The nearest ancestor of `file` holding a `Cargo.toml`, bounded by the repo.
///
/// Not simply "the repo root": plenty of repos keep a Rust component in a
/// subdirectory next to services in other languages, and cargo must run where
/// the manifest is. `--workspace` then covers every member from that point, so
/// one invocation per manifest root is enough.
fn cargo_root_for(root: &str, file: &str) -> Option<PathBuf> {
    let mut dir = Path::new(root).join(file);
    dir.pop();
    loop {
        if dir.join("Cargo.toml").is_file() {
            return Some(dir);
        }
        if dir == Path::new(root) || !dir.starts_with(root) {
            return None;
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn cargo_roots<'a>(root: &str, files: impl Iterator<Item = &'a str>) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    for f in files.filter(|f| is_rust_path(f)) {
        if let Some(d) = cargo_root_for(root, f) {
            seen.insert(d);
        }
    }
    seen.into_iter().collect()
}

/// True when `cargo <component> --version` works.
///
/// Only for separately-installable COMPONENTS — rustfmt and clippy, which a
/// toolchain can legitimately lack. A missing one must warn and pass, never
/// fail a commit for a tool the developer never chose.
///
/// Do NOT probe a BUILT-IN subcommand this way: `cargo test --version` is
/// "unexpected argument '--version'", so the probe reports test as unavailable
/// and the gate silently passes — it would never have run a test anywhere.
fn component_available(dir: &Path, sub: &str) -> bool {
    let Some(cargo) = which("cargo") else {
        return false;
    };
    Command::new(cargo)
        .arg(sub)
        .arg("--version")
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn cargo_argv() -> Option<Vec<String>> {
    which("cargo").map(|c| vec![c])
}

/// Shared shape: find the manifest roots, verify the component if there is one
/// to verify, run the command in each, report once.
///
/// `component` is `Some` only for rustfmt/clippy; `None` means a built-in
/// subcommand where cargo's own presence is the whole requirement.
fn each_root(
    roots: &[PathBuf],
    component: Option<&str>,
    args: &[&str],
    missing: &str,
) -> Option<bool> {
    let argv = cargo_argv().or_else(|| {
        warn(missing);
        None
    })?;
    let mut all_ok = true;
    for dir in roots {
        if let Some(c) = component {
            if !component_available(dir, c) {
                warn(missing);
                return None;
            }
        }
        let extra: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        let d = dir.to_string_lossy().into_owned();
        if !run_tool(&d, &argv, &extra) {
            all_ok = false;
        }
    }
    Some(all_ok)
}

pub fn fmt(_args: &[std::ffi::OsString]) -> Outcome {
    let files = staged_files(&[".rs"]);
    if files.is_empty() {
        return Outcome::Passed;
    }
    let root = repo_root();
    let roots = cargo_roots(&root, files.iter().map(String::as_str));
    if roots.is_empty() {
        return Outcome::Passed;
    }
    // `--all -- --check` per the project convention. Note this inspects the
    // WORKING TREE, not the index, so a partially-staged file is judged by its
    // unstaged form too. Same trade-off cargo fmt gives everyone; scoping it to
    // staged paths would need the edition resolved by hand.
    match each_root(
        &roots,
        Some("fmt"),
        &["fmt", "--all", "--", "--check"],
        "Rust staged but rustfmt is not installed. `rustup component add rustfmt`.",
    ) {
        None => Outcome::Unavailable,
        Some(true) => {
            ok("Rust formatting is clean");
            Outcome::Passed
        }
        Some(false) => {
            fail(&format!("Unformatted Rust. Run {}.", hl("cargo fmt --all")));
            Outcome::Failed
        }
    }
}

pub fn clippy(_args: &[std::ffi::OsString]) -> Outcome {
    // `staged_files` matches by suffix, which would also accept
    // `vendor/NotCargo.toml`. `is_rust_path` compares the basename, so let it
    // be the only filter rather than keeping two that disagree.
    let files: Vec<String> = staged_files(&[])
        .into_iter()
        .filter(|f| is_rust_path(f))
        .collect();
    if files.is_empty() {
        return Outcome::Passed;
    }
    let root = repo_root();
    let roots = cargo_roots(&root, files.iter().map(String::as_str));
    if roots.is_empty() {
        return Outcome::Passed;
    }
    match each_root(
        &roots,
        Some("clippy"),
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ],
        "Rust staged but clippy is not installed. `rustup component add clippy`.",
    ) {
        None => Outcome::Unavailable,
        Some(true) => {
            ok("Clippy passed");
            Outcome::Passed
        }
        Some(false) => {
            fail(&format!(
                "Clippy warnings. Fix them or run {}.",
                hl("cargo clippy --fix")
            ));
            Outcome::Failed
        }
    }
}

/// pre-push. Mirrors `run-tests-js`: the range that is actually being pushed
/// decides whether the suite runs, so a docs-only push costs nothing.
pub fn test(refs: &[crate::pushrefs::PushRef]) -> Outcome {
    let Some(root) = git::stdout(&["rev-parse", "--show-toplevel"]) else {
        return Outcome::Passed;
    };
    let changed = crate::pushrefs::changed_files(refs);
    let roots = cargo_roots(&root, changed.iter().map(String::as_str));
    if roots.is_empty() {
        return Outcome::Passed;
    }
    match each_root(
        &roots,
        None,
        &["test", "--workspace", "--all-features"],
        "Rust changed but cargo is not installed.",
    ) {
        None => Outcome::Unavailable,
        Some(true) => {
            ok("Rust tests passed");
            Outcome::Passed
        }
        Some(false) => {
            fail("Rust tests failed. Push aborted.");
            Outcome::Failed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_rust_paths() {
        assert!(is_rust_path("src/main.rs"));
        assert!(is_rust_path("Cargo.toml"));
        assert!(is_rust_path("crates/a/Cargo.lock"));
        assert!(is_rust_path("rustfmt.toml"));
        assert!(!is_rust_path("README.md"));
        assert!(!is_rust_path("src/main.rsx"));
        // A file merely CONTAINING the name is not the manifest.
        assert!(!is_rust_path("docs/Cargo.toml.md"));
        assert!(!is_rust_path("vendor/NotCargo.toml"));
    }

    #[test]
    fn finds_the_nearest_manifest_not_the_repo_root() {
        let tmp = std::env::temp_dir().join("githooks-cargo-roots");
        let _ = std::fs::remove_dir_all(&tmp);
        let nested = tmp.join("services/engine");
        std::fs::create_dir_all(nested.join("src")).unwrap();
        std::fs::write(nested.join("Cargo.toml"), "[package]\n").unwrap();
        let root = tmp.to_string_lossy().into_owned();

        let got = cargo_roots(&root, ["services/engine/src/main.rs"].into_iter());
        assert_eq!(got, vec![nested.clone()], "should find the nested manifest");

        // A Rust file with no manifest anywhere above it is not a cargo project.
        std::fs::create_dir_all(tmp.join("scripts")).unwrap();
        let none = cargo_roots(&root, ["scripts/loose.rs"].into_iter());
        assert!(none.is_empty(), "no manifest above it: {none:?}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn several_files_in_one_crate_yield_one_root() {
        let tmp = std::env::temp_dir().join("githooks-cargo-dedupe");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src")).unwrap();
        std::fs::write(tmp.join("Cargo.toml"), "[package]\n").unwrap();
        let root = tmp.to_string_lossy().into_owned();
        let got = cargo_roots(&root, ["src/a.rs", "src/b.rs", "Cargo.toml"].into_iter());
        assert_eq!(got.len(), 1, "one cargo invocation, not three: {got:?}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn non_rust_files_select_nothing() {
        let got = cargo_roots("/tmp", ["README.md", "a.py"].into_iter());
        assert!(got.is_empty());
    }
}
