//! What the Rust `FixPlan` removes, pinned as a GOLDEN expectation.
//!
//! This began as a differential against `scripts/propagate.sh --dry-run`. That
//! comparison is what earned the right to delete the shell script — until the
//! two agreed, the Rust apply path was an untested rewrite of something that
//! had already destroyed tracked files twice.
//!
//! The script is now gone, so the comparison is against the set it produced,
//! recorded here. Deleting the gate along with the script would have thrown
//! away the only evidence the rewrite was faithful; keeping the expectation
//! keeps the behaviour pinned without keeping the shell.
//!
//! It runs on a SYNTHETIC fleet, deliberately. The real fleet is currently
//! clean, so comparing the two implementations over it would compare an empty
//! set to an empty set and pass while proving nothing — the same shape as the
//! `0 copies / 0 distinct` sweep that started this whole exercise. So the tree
//! below contains one repo for each condition either implementation can act on,
//! and the test asserts up front that the comparison is not vacuous.

use std::collections::BTreeSet;
use std::path::PathBuf;
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
            "#!/bin/sh\n# git-templates hook shim.\nexec x --hooks-dir y pre-commit-ruff\n",
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

    /// The removals `propagate.sh` produced for this exact fixture, recorded
    /// from its last run before it was deleted. Relative to the fleet root so
    /// the expectation survives a different temp directory.
    fn golden_removals(&self) -> BTreeSet<String> {
        [
            "foreign/.git/hooks/pre-push-branch-protect.sh",
            "pkg/.git/hooks/package.json",
            "stale/.git/hooks/pre-commit-ruff",
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    }

    /// Rust removals, made root-relative so they compare with the golden set.
    fn rust_removals(&self) -> BTreeSet<String> {
        let out = Command::new(env!("CARGO_BIN_EXE_githooks-fleet"))
            .args(["fix", "--json", "--root"])
            .arg(&self.root)
            .arg("--binary")
            .arg(&self.binary)
            .output()
            .expect("githooks-fleet");
        let plans: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json");
        plans
            .as_array()
            .expect("array")
            .iter()
            .flat_map(|p| p["remove"].as_array().cloned().unwrap_or_default())
            .map(|r| {
                let p = r["path"].as_str().unwrap_or_default();
                let root = self.root.to_string_lossy();
                p.strip_prefix(root.as_ref())
                    .unwrap_or(p)
                    .trim_start_matches(std::path::MAIN_SEPARATOR)
                    .replace(std::path::MAIN_SEPARATOR, "/")
            })
            .collect()
    }
}

impl Drop for Fleet {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn the_plan_removes_exactly_the_recorded_set() {
    let f = Fleet::new("parity");
    f.healthy("clean")
        .with_stale_ours("stale")
        .with_foreign_sub("foreign")
        .with_pkgjson("pkg")
        .unmanaged("legacy")
        .missing_a_shim("incomplete");

    let golden = f.golden_removals();
    let rust = f.rust_removals();

    // Guard against a vacuous pass: if the fixture stops producing removals
    // this proves nothing, which is how the original sweep reported a clean
    // fleet it had never looked at.
    assert!(
        golden.len() >= 3,
        "expectation is too small to mean anything"
    );

    let missing: Vec<_> = golden.difference(&rust).collect();
    let extra: Vec<_> = rust.difference(&golden).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "plan drifted from the recorded behaviour.\n  not removed: {missing:?}\n  unexpected: {extra:?}"
    );
}

/// Neither implementation may touch a repo it does not manage. That rule is the
/// only thing standing between this tool and an application's data repository.
#[test]
fn neither_implementation_touches_an_unmanaged_repo() {
    let f = Fleet::new("unmanaged");
    f.healthy("ok").unmanaged("legacy");

    for path in f.rust_removals() {
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

/// End to end through the CLI, and then AGAIN to prove idempotence.
///
/// Note the explicit `--root`. The version of this test that came before it
/// invoked `fix --apply` with no root, so it fell back to $HOME/Developer and
/// ran the apply path over the real fleet — 13 seconds of it. That was harmless
/// only because every plan there is currently a no-op. A test must never be one
/// clean fleet away from mutating the machine it runs on.
#[test]
fn apply_removes_the_stale_file_and_is_idempotent() {
    let f = Fleet::new("apply");
    f.with_stale_ours("stale").healthy("clean");
    let stale = f.root.join("stale/.git/hooks/pre-commit-ruff");
    assert!(stale.exists(), "guard");

    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_githooks-fleet"))
            .args(args)
            .arg("--root")
            .arg(&f.root)
            .arg("--binary")
            .arg(&f.binary)
            .output()
            .expect("run")
    };

    let first = run(&["fix", "--apply", "--json"]);
    assert!(first.status.success());
    assert!(!stale.exists(), "the stale file must be gone");

    let v: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    let applied: Vec<_> = v
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["outcome"]["outcome"] == "applied")
        .collect();
    assert_eq!(applied.len(), 1, "only the stale repo changed: {v}");

    // Second run: nothing left to do.
    let second = run(&["fix", "--apply", "--json"]);
    let v2: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert!(
        v2.as_array()
            .unwrap()
            .iter()
            .all(|r| r["outcome"]["outcome"] != "applied"),
        "applying twice must change nothing the second time: {v2}"
    );
}

/// An unmanaged repo survives `--apply` untouched. This is the rule that keeps
/// the tool away from an application's data repository.
#[test]
fn apply_leaves_an_unmanaged_repo_alone() {
    let f = Fleet::new("applyskip");
    f.healthy("ok").unmanaged("legacy");
    let legacy = f.root.join("legacy/.git/hooks/pre-commit");
    let before = std::fs::read_to_string(&legacy).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_githooks-fleet"))
        .args(["fix", "--apply", "--json", "--root"])
        .arg(&f.root)
        .arg("--binary")
        .arg(&f.binary)
        .output()
        .expect("run");
    assert!(out.status.success());
    assert_eq!(
        std::fs::read_to_string(&legacy).unwrap(),
        before,
        "an unmanaged repo must be byte-identical afterwards"
    );
    assert!(
        f.root
            .join("legacy/.git/hooks/pre-commit-ban-terms.js")
            .exists(),
        "including its legacy sub-hooks"
    );
}
