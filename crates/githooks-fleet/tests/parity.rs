//! The parity gate: the Rust `FixPlan` against `scripts/propagate.sh --dry-run`.
//!
//! This is what earns the right to delete the shell script. Until the two agree
//! on what would be removed, the Rust apply path is an untested rewrite of
//! something that has already destroyed tracked files twice.
//!
//! It runs on a SYNTHETIC fleet, deliberately. The real fleet is currently
//! clean, so comparing the two implementations over it would compare an empty
//! set to an empty set and pass while proving nothing — the same shape as the
//! `0 copies / 0 distinct` sweep that started this whole exercise. So the tree
//! below contains one repo for each condition either implementation can act on,
//! and the test asserts up front that the comparison is not vacuous.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn template() -> String {
    std::fs::read_to_string(repo_root().join("templates/hooks/pre-commit")).expect("template")
}

fn shim_for(binary: &str) -> String {
    template().replace("__GITHOOKS_BIN__", binary)
}

const DISPATCHERS: [&str; 4] = ["commit-msg", "pre-commit", "pre-push", "prepare-commit-msg"];

struct Fleet {
    root: PathBuf,
    binary: PathBuf,
}

impl Fleet {
    /// `name` must be unique per test. `std::process::id()` is NOT enough:
    /// cargo runs the tests in one binary on several threads, so two fixtures
    /// derived from the pid share a directory and delete each other's tree
    /// mid-run. That produced a "disagreement" that was really one test wiping
    /// the other's files between the two implementations reading them.
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("fleet-parity-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("mkdir");
        // propagate.sh insists the binary exists before it will do anything.
        let binary = root.join("fake-githooks");
        std::fs::write(&binary, "#!/bin/sh\nexit 0\n").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Fleet { root, binary }
    }

    fn hooks(&self, rel: &str) -> PathBuf {
        let h = self.root.join(rel).join(".git/hooks");
        std::fs::create_dir_all(&h).expect("mkdir");
        h
    }

    /// A managed repo: all four dispatchers, correctly baked.
    fn healthy(&self, rel: &str) -> &Self {
        let h = self.hooks(rel);
        for n in DISPATCHERS {
            std::fs::write(h.join(n), shim_for(self.binary.to_str().unwrap())).unwrap();
        }
        self
    }

    /// Managed, but carrying a retired per-check shim of ours.
    fn with_stale_ours(&self, rel: &str) -> &Self {
        self.healthy(rel);
        std::fs::write(
            self.hooks(rel).join("pre-commit-ruff"),
            "#!/bin/sh\nexec x --hooks-dir y pre-commit-ruff\n",
        )
        .unwrap();
        self
    }

    /// Managed, carrying somebody's hand-written sub-hook that nothing runs.
    fn with_foreign_sub(&self, rel: &str) -> &Self {
        self.healthy(rel);
        std::fs::write(
            self.hooks(rel).join("pre-push-branch-protect.sh"),
            "#!/bin/sh\necho mine\n",
        )
        .unwrap();
        self
    }

    /// Managed, carrying the node-era package.json.
    fn with_pkgjson(&self, rel: &str) -> &Self {
        self.healthy(rel);
        std::fs::write(
            self.hooks(rel).join("package.json"),
            "{\n  \"type\": \"commonjs\",\n  \"//\": \"Forces Node to treat...\"\n}\n",
        )
        .unwrap();
        self
    }

    /// NOT managed: pre-migration hooks, no dispatch to the binary. Both
    /// implementations must leave it entirely alone. This is the case that
    /// covers both an app data repo and a repo that predates the migration.
    fn unmanaged(&self, rel: &str) -> &Self {
        let h = self.hooks(rel);
        std::fs::write(h.join("pre-commit"), "#!/bin/zsh\necho legacy\n").unwrap();
        std::fs::write(h.join("pre-commit-ban-terms.js"), "// legacy\n").unwrap();
        self
    }

    /// Managed but missing a dispatcher — a write, no removals.
    fn missing_a_shim(&self, rel: &str) -> &Self {
        self.healthy(rel);
        std::fs::remove_file(self.hooks(rel).join("pre-push")).unwrap();
        self
    }

    fn propagate_removals(&self) -> BTreeSet<String> {
        let out = Command::new("sh")
            .arg(repo_root().join("scripts/propagate.sh"))
            .env("ROOT", &self.root)
            .env("GITHOOKS_BIN", &self.binary)
            .output()
            .expect("propagate.sh");
        assert!(
            out.status.success(),
            "propagate.sh failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.trim().strip_prefix("would rm  "))
            .map(|p| p.split("  (").next().unwrap_or(p).trim().to_string())
            .collect()
    }

    fn rust_removals(&self) -> BTreeSet<String> {
        let out = Command::new(env!("CARGO_BIN_EXE_githooks-fleet"))
            .args(["fix", "--json", "--root"])
            .arg(&self.root)
            .output()
            .expect("githooks-fleet");
        let plans: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json");
        plans
            .as_array()
            .expect("array")
            .iter()
            .flat_map(|p| p["remove"].as_array().cloned().unwrap_or_default())
            .map(|r| r["path"].as_str().unwrap_or_default().to_string())
            .collect()
    }
}

impl Drop for Fleet {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Normalise for comparison: the shell prints paths as it found them, and macOS
/// hands out `/var` symlinks to `/private/var`.
fn norm(set: BTreeSet<String>) -> BTreeSet<String> {
    set.into_iter()
        .map(|p| {
            Path::new(&p)
                .canonicalize()
                .map(|c| c.to_string_lossy().into_owned())
                .unwrap_or(p)
        })
        .collect()
}

#[test]
fn the_rust_plan_removes_exactly_what_the_shell_sweep_removes() {
    let f = Fleet::new("parity");
    f.healthy("clean")
        .with_stale_ours("stale")
        .with_foreign_sub("foreign")
        .with_pkgjson("pkg")
        .unmanaged("legacy")
        .missing_a_shim("incomplete");

    let shell = norm(f.propagate_removals());
    let rust = norm(f.rust_removals());

    // Guard against a vacuous pass. If the fixture stops producing removals,
    // this comparison proves nothing — which is exactly how the original sweep
    // reported a clean fleet it had never looked at.
    assert!(
        shell.len() >= 3,
        "fixture produced too few removals to be a real comparison: {shell:?}"
    );

    let only_shell: Vec<_> = shell.difference(&rust).collect();
    let only_rust: Vec<_> = rust.difference(&shell).collect();
    assert!(
        only_shell.is_empty() && only_rust.is_empty(),
        "plans disagree.\n  only propagate.sh: {only_shell:?}\n  only Rust: {only_rust:?}"
    );
}

/// Neither implementation may touch a repo it does not manage. That rule is the
/// only thing standing between this tool and an application's data repository.
#[test]
fn neither_implementation_touches_an_unmanaged_repo() {
    let f = Fleet::new("unmanaged");
    f.healthy("ok").unmanaged("legacy");

    for path in norm(f.propagate_removals())
        .iter()
        .chain(norm(f.rust_removals()).iter())
    {
        assert!(
            !path.contains("legacy"),
            "an unmanaged repo must be left alone: {path}"
        );
    }

    let out = Command::new(env!("CARGO_BIN_EXE_githooks-fleet"))
        .args(["fix", "--json", "--root"])
        .arg(&f.root)
        .output()
        .expect("run");
    let plans: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let legacy = plans
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["repo"].as_str().unwrap_or_default().contains("legacy"))
        .expect("legacy repo planned");
    assert_eq!(legacy["refuse"][0]["refusal"], "unmanaged");
    assert!(legacy["write"].as_array().unwrap().is_empty());
}

/// `fix` is a dry run in this release, and must say so rather than implying it
/// acted.
#[test]
fn fix_writes_nothing() {
    let f = Fleet::new("drywrite");
    f.with_stale_ours("stale");
    let stale = f.root.join("stale/.git/hooks/pre-commit-ruff");
    assert!(stale.exists());

    let out = Command::new(env!("CARGO_BIN_EXE_githooks-fleet"))
        .args(["fix", "--root"])
        .arg(&f.root)
        .output()
        .expect("run");
    assert!(stale.exists(), "dry run must not delete");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("DRY RUN"),
        "and must say so"
    );
}

#[test]
fn apply_is_rejected_until_it_exists() {
    let out = Command::new(env!("CARGO_BIN_EXE_githooks-fleet"))
        .args(["fix", "--apply"])
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not implemented"));
}
