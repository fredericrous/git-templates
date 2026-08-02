//! Scanner behaviour, driven through the real binary.
//!
//! The cases that matter are the ones where the old shell sweep was silent: a
//! root that is wrong, a depth that is too shallow, a directory that cannot be
//! read. Each has to be distinguishable from "the fleet is clean", which is the
//! single failure this tool exists to prevent.

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_githooks-fleet")
}

struct Tree(PathBuf);

impl Tree {
    fn new(name: &str) -> Self {
        let d = std::env::temp_dir().join(format!("fleet-scan-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("mkdir");
        Tree(d)
    }
    /// A repo whose hooks dispatch to the binary, i.e. one of ours.
    fn managed_repo(&self, rel: &str) -> &Self {
        let hooks = self.0.join(rel).join(".git/hooks");
        std::fs::create_dir_all(&hooks).expect("mkdir");
        std::fs::write(
            hooks.join("pre-commit"),
            "#!/bin/sh\nexec \"$BIN\" --hooks-dir \"$(dirname \"$0\")\" pre-commit \"$@\"\n",
        )
        .expect("write");
        self
    }
    /// A git repo we do not manage — no shim dispatching to the binary.
    fn foreign_repo(&self, rel: &str) -> &Self {
        let hooks = self.0.join(rel).join(".git/hooks");
        std::fs::create_dir_all(&hooks).expect("mkdir");
        std::fs::write(hooks.join("pre-commit"), "#!/bin/sh\necho mine\n").expect("write");
        self
    }
    /// A REAL repository (`managed_repo`/`foreign_repo` only fake the
    /// directory shape) whose `core.hooksPath` points somewhere other than
    /// `.git/hooks`, with a managed shim living at that redirected location
    /// and nothing at the default one.
    fn custom_hooks_path_repo(&self, rel: &str, hooks_rel: &str) -> &Self {
        let repo = self.0.join(rel);
        std::fs::create_dir_all(&repo).expect("mkdir");
        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .expect("git");
        };
        git(&["init", "-q", "--template=", "."]);
        git(&["config", "core.hooksPath", hooks_rel]);
        let hooks = repo.join(hooks_rel);
        std::fs::create_dir_all(&hooks).expect("mkdir");
        std::fs::write(
            hooks.join("pre-commit"),
            "#!/bin/sh\nexec \"$BIN\" --hooks-dir \"$(dirname \"$0\")\" pre-commit \"$@\"\n",
        )
        .expect("write");
        self
    }
    /// `core.hooksPath` set via an INCLUDED file rather than directly in
    /// `.git/config` — the case a local-text-only shortcut would miss, since
    /// the literal string `hooksPath` never appears in `.git/config` itself.
    fn hooks_path_via_include_repo(&self, rel: &str, hooks_rel: &str) -> &Self {
        let repo = self.0.join(rel);
        std::fs::create_dir_all(&repo).expect("mkdir");
        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .expect("git");
        };
        git(&["init", "-q", "--template=", "."]);
        std::fs::write(
            repo.join("shared.gitconfig"),
            format!("[core]\n\thooksPath = {hooks_rel}\n"),
        )
        .expect("write shared config");
        git(&["config", "--add", "include.path", "../shared.gitconfig"]);
        let hooks = repo.join(hooks_rel);
        std::fs::create_dir_all(&hooks).expect("mkdir");
        std::fs::write(
            hooks.join("pre-commit"),
            "#!/bin/sh\nexec \"$BIN\" --hooks-dir \"$(dirname \"$0\")\" pre-commit \"$@\"\n",
        )
        .expect("write");
        self
    }
    fn dir(&self, rel: &str) -> &Self {
        std::fs::create_dir_all(self.0.join(rel)).expect("mkdir");
        self
    }
    /// A committed `.githooks.conf` at a repo root.
    fn manifest(&self, rel: &str, body: &str) -> &Self {
        std::fs::write(self.0.join(rel).join(".githooks.conf"), body).expect("write");
        self
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Run {
    code: i32,
    stdout: String,
}

fn run(args: &[&str]) -> Run {
    let out = Command::new(bin()).args(args).output().expect("run");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
    }
}

fn json(args: &[&str]) -> serde_json::Value {
    let mut a = args.to_vec();
    a.push("--json");
    serde_json::from_str(&run(&a).stdout).expect("valid json")
}

#[test]
fn counts_managed_and_unmanaged_separately() {
    let t = Tree::new("mixed");
    t.managed_repo("a").managed_repo("b/c").foreign_repo("d");
    let v = json(&["--root", t.path().to_str().unwrap()]);
    assert_eq!(v["git_dirs_found"], 3);
    assert_eq!(v["managed_seen"], 2);
    assert_eq!(v["unmanaged_seen"], 1);
    assert_eq!(v["repos"].as_array().unwrap().len(), 3);
}

/// THE case. A wrong root must be loud in three independent ways: a non-zero
/// exit, the words "SCAN FAILURE", and counters that show nothing was examined.
/// Any one of them alone could be missed.
#[test]
fn a_wrong_root_is_a_failure_not_an_empty_fleet() {
    let t = Tree::new("empty");
    t.dir("nothing/here");
    let r = run(&["--root", t.path().to_str().unwrap()]);
    assert_eq!(r.code, 1, "an empty scan must not exit 0");
    assert!(
        r.stdout.contains("SCAN FAILURE"),
        "must say so in words: {}",
        r.stdout
    );
    assert!(
        r.stdout.contains("--root") && r.stdout.contains("--depth"),
        "must name the two things worth checking: {}",
        r.stdout
    );
    let v = json(&["--root", t.path().to_str().unwrap()]);
    assert_eq!(v["git_dirs_found"], 0);
    assert!(v["dirs_visited"].as_u64().unwrap() > 0, "it did look");
}

/// A depth too shallow to reach the repos is the same failure wearing a
/// different hat, and it is the one that produced `0 copies / 0 distinct`.
#[test]
fn a_too_shallow_depth_finds_nothing_and_says_so() {
    let t = Tree::new("deep");
    t.managed_repo("one/two/three/four");
    let deep = json(&["--root", t.path().to_str().unwrap(), "--depth", "6"]);
    assert_eq!(deep["git_dirs_found"], 1, "reachable at depth 6");

    let r = run(&["--root", t.path().to_str().unwrap(), "--depth", "1"]);
    assert_eq!(r.code, 1);
    assert!(r.stdout.contains("SCAN FAILURE"), "{}", r.stdout);
    assert!(
        r.stdout.contains("currently: 1"),
        "must show the depth it used: {}",
        r.stdout
    );
}

#[test]
fn excluded_subtrees_are_skipped_and_counted() {
    let t = Tree::new("excluded");
    t.managed_repo("app");
    t.managed_repo("app/node_modules/pkg");
    let v = json(&["--root", t.path().to_str().unwrap()]);
    assert_eq!(v["git_dirs_found"], 1, "the vendored repo must not count");
    assert!(
        v["excluded_dirs"].as_u64().unwrap() >= 1,
        "and the skip must be recorded, not silent"
    );
}

/// Repos are reported relative to the root, because that is what a human
/// recognises, and sorted so output is stable between runs.
#[test]
fn paths_are_root_relative_and_sorted() {
    let t = Tree::new("sorted");
    t.managed_repo("zeta")
        .managed_repo("alpha")
        .managed_repo("mid");
    let v = json(&["--root", t.path().to_str().unwrap()]);
    let paths: Vec<&str> = v["repos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["path"].as_str().unwrap())
        .collect();
    // Single components, so no separator appears — safe to compare literally.
    assert_eq!(paths, vec!["alpha", "mid", "zeta"]);
}

#[test]
fn a_root_that_does_not_exist_is_rejected_before_scanning() {
    let r = run(&["--root", "/definitely/not/here"]);
    assert_eq!(r.code, 2, "usage error, distinct from an empty scan");
}

/// The JSON is the contract the TUI renders over, so its shape is pinned.
#[test]
fn json_carries_every_counter() {
    let t = Tree::new("shape");
    t.managed_repo("a");
    let v = json(&["--root", t.path().to_str().unwrap()]);
    for k in [
        "root",
        "depth",
        "git_dirs_found",
        "hook_dirs_seen",
        "managed_seen",
        "unmanaged_seen",
        "unreadable",
        "excluded_dirs",
        "dirs_visited",
        "repos",
    ] {
        assert!(!v[k].is_null(), "missing {k} from the scan contract");
    }
}

/// A repo four directories down must be found at the default depth.
///
/// Not hypothetical: `Perso/jellyfish/tuna/Coinbase-OAuth2` sat at that depth
/// and was invisible to every sweep, because `propagate.sh` and the hand-run
/// verification both used `find -maxdepth 6` while its hook files are eight
/// path components deep. It is still running pre-migration zsh hooks. The whole
/// fleet was reported consistent while one repo had never been touched.
#[test]
fn a_deeply_nested_repo_is_found_at_default_depth() {
    let t = Tree::new("nested");
    t.managed_repo("Perso/group/project/sub");
    let v = json(&["--root", t.path().to_str().unwrap()]);
    assert_eq!(v["git_dirs_found"], 1, "default depth must reach it");
    // Built through PathBuf: paths serialise with the platform's separator, so
    // a hardcoded "a/b" passes on Unix and fails on Windows for no real reason.
    let expected = PathBuf::from("Perso")
        .join("group")
        .join("project")
        .join("sub");
    assert_eq!(v["repos"][0]["path"], expected.to_string_lossy().as_ref());
}

/// Files nothing dispatches any more: they look installed and never run.
#[test]
fn stale_and_foreign_hook_files_are_reported_separately() {
    let t = Tree::new("leftovers");
    t.managed_repo("a");
    let hooks = t.path().join("a/.git/hooks");
    // Ours, but no longer shipped (a retired per-check shim).
    std::fs::write(
        hooks.join("pre-commit-ruff"),
        "#!/bin/sh\nexec x --hooks-dir y pre-commit-ruff\n",
    )
    .unwrap();
    // Somebody's own sub-hook: not ours, and now dispatched by nothing.
    std::fs::write(hooks.join("pre-push-mine.sh"), "#!/bin/sh\necho hi\n").unwrap();
    std::fs::write(hooks.join("package.json"), "{\"//\":\"Forces Node\"}").unwrap();

    let v = json(&["--root", t.path().to_str().unwrap()]);
    let r = &v["repos"][0];
    assert_eq!(r["stale_ours"][0], "pre-commit-ruff");
    assert_eq!(r["foreign_subs"][0], "pre-push-mine.sh");
    assert_eq!(r["hook_pkgjson"], true);
}

/// A shim installed by `make install` must classify as OK, not drifted — the
/// failure that would make the tool cry wolf on all 96 repos.
#[test]
fn a_baked_shim_is_ok_and_a_hand_edited_one_is_not() {
    let t = Tree::new("baked");
    let hooks = t.path().join("r/.git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let template = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../templates/hooks/pre-commit"
    ))
    .unwrap();
    for n in ["commit-msg", "pre-commit", "pre-push", "prepare-commit-msg"] {
        std::fs::write(
            hooks.join(n),
            template.replace("__GITHOOKS_BIN__", "/opt/githooks"),
        )
        .unwrap();
    }
    let v = json(&["--root", t.path().to_str().unwrap(), "--depth", "3"]);
    let r = &v["repos"][0];
    assert!(
        r["shims"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["state"] == "ok"),
        "baked shims must not read as drift: {:?}",
        r["shims"]
    );
    assert_eq!(
        r["baked"]["bake"], "stale",
        "and /opt is not where we install"
    );

    std::fs::write(hooks.join("pre-push"), "#!/bin/sh\necho tampered\n").unwrap();
    let v = json(&["--root", t.path().to_str().unwrap(), "--depth", "3"]);
    let states: Vec<&str> = v["repos"][0]["shims"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["state"].as_str().unwrap())
        .collect();
    assert!(
        states.contains(&"drifted"),
        "real drift must show: {states:?}"
    );
}

/// Declared checks were invisible to the fleet view: a repository could run a
/// command on every commit that no column mentioned. The scanner reads the same
/// file the dispatcher does, with the same parser.
#[test]
fn a_declared_check_reaches_the_scan() {
    let t = Tree::new("declared");
    t.managed_repo("a").manifest(
        "a",
        "pre-commit  shellcheck  *.sh  block  scripts/lint-shell.sh\n",
    );
    let v = json(&["--root", t.path().to_str().unwrap()]);
    let d = &v["repos"][0]["declared"][0];
    assert_eq!(d["name"], "shellcheck");
    assert_eq!(d["stage"], "pre-commit");
    // A tagged sum, so the usable fields exist only when the line is usable.
    assert_eq!(d["state"], "usable");
    assert_eq!(d["severity"], "block");
    assert_eq!(d["command"], "scripts/lint-shell.sh");
    assert_eq!(d["exts"][0], ".sh");
}

/// The case worth scanning ninety-six repositories for: a line committed
/// months ago that has never once run.
#[test]
fn an_unusable_declaration_is_reported_as_such() {
    let t = Tree::new("declared-broken");
    t.managed_repo("a")
        .manifest("a", "pre-commit  shellcheck  *.sh  LOUD  make lint\n");
    let v = json(&["--root", t.path().to_str().unwrap()]);
    let d = &v["repos"][0]["declared"][0];
    assert_eq!(d["state"], "unusable", "{d}");
    let why = d["why"].as_str().expect("must say why");
    assert!(why.contains("severity"), "{why}");
    assert!(why.contains("line 1"), "{why}");
    // And it carries no command, because the type has nowhere to put one.
    assert!(d["command"].is_null(), "{d}");
}

/// Ninety-six repositories have no manifest, and that must cost nothing and
/// produce no rows.
#[test]
fn a_repo_without_a_manifest_declares_nothing() {
    let t = Tree::new("declared-none");
    t.managed_repo("a");
    let v = json(&["--root", t.path().to_str().unwrap()]);
    assert_eq!(
        v["repos"][0]["declared"].as_array().map(Vec::len),
        Some(0),
        "{}",
        v["repos"][0]
    );
}

/// A repo can point `core.hooksPath` anywhere; the scanner must look there,
/// not at `.git/hooks`. Otherwise a redirected repo reads as unmanaged (the
/// shim IS there, just not where this tool assumed) and `fix --apply` would
/// go on to WRITE at a path git never executes.
#[test]
fn core_hooks_path_is_honoured_not_assumed() {
    let t = Tree::new("hookspath");
    t.custom_hooks_path_repo("redirected", "tooling/hooks");
    let v = json(&["--root", t.path().to_str().unwrap()]);
    let repo = &v["repos"][0];
    assert_eq!(repo["managed"], true, "{repo}");
    // The default location was never created — a managed reading here would
    // mean the scan fell back to `.git/hooks` and got lucky, not that it
    // resolved `core.hooksPath`.
    assert!(!t.path().join("redirected/.git/hooks").exists());
}

/// `core.hooksPath` reaching a repo through an `include`, not a literal key
/// in its own `.git/config` — a local-file-only shortcut would see nothing
/// to act on here and fall back to `.git/hooks`, wrongly.
#[test]
fn core_hooks_path_via_an_include_is_still_honoured() {
    let t = Tree::new("hookspath-include");
    t.hooks_path_via_include_repo("redirected", "tooling/hooks");
    let v = json(&["--root", t.path().to_str().unwrap()]);
    let repo = &v["repos"][0];
    assert_eq!(repo["managed"], true, "{repo}");
    assert!(!t.path().join("redirected/.git/hooks").exists());
}
