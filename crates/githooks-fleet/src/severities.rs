//! Reading `githooks.severity.<check>`, the quieter cousin of `hook.skip`.
//!
//! A skip is at least honest about itself: the check does not run, the
//! dispatcher says so on every commit, and the dashboard counts it. A severity
//! downgrade leaves everything looking normal — the check runs, prints its
//! failure in red, and the commit sails through. Read the terminal and you see
//! a hook doing its job. Read the exit code and it enforced nothing.
//!
//! That gap is the whole reason this module exists. A repo with six downgraded
//! checks and a repo with none are indistinguishable in every column the
//! dashboard had before, and the six-downgrade repo is the one worth knowing
//! about.
//!
//! Read-only, like `skips` was at first. Toggling severity from the UI is a
//! later change; being able to SEE it does not have to wait for that.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::checks::all_checks;
use crate::skips::{scope_of, Scope};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Warn,
    Block,
    /// Anything else in the config. Git will not complain about it and neither
    /// will the hook — it silently falls back to the declared severity. Worth
    /// showing precisely because it looks like it did something.
    Unrecognised,
}

impl Level {
    /// Read through the RUNTIME's parser, never a copy of it. This type exists
    /// only because the dashboard needs a third state — `Unrecognised` — that
    /// the dispatcher does not: to the hook an unknown value is simply absent,
    /// while here it is a line somebody wrote that does nothing.
    fn of(value: &str) -> Level {
        match githooks_runtime::check::Severity::parse(value) {
            Some(githooks_runtime::check::Severity::Warn) => Level::Warn,
            Some(githooks_runtime::check::Severity::Block) => Level::Block,
            None => Level::Unrecognised,
        }
    }
}

/// A configured line, before anyone has asked which one git applies.
///
/// Carries only what the config file says, so there is no state in which a
/// field is meaningless.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RawEntry {
    check: String,
    value: String,
    scope: Scope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SeverityOverride {
    /// The key's last component, exactly as configured.
    pub check: String,
    /// The configured value, exactly as written.
    pub value: String,
    pub level: Level,
    pub scope: Scope,
    /// Whether this is the entry git would actually apply.
    ///
    /// `--get-regexp` lists EVERY entry; the dispatcher asks `--get`, which
    /// returns the last one, so a local `block` beats a global `warn`. Listing
    /// both and treating each as authoritative made the dashboard report a
    /// downgrade the dispatcher does not apply — measured, with global `warn`
    /// and local `block` on one check.
    ///
    /// Shadowed entries are still SHOWN. Somebody wrote them, and knowing a
    /// global downgrade is being overridden here is worth more than a tidy list.
    pub effective: bool,
}

impl SeverityOverride {
    /// True when the key names no check this binary knows. Unlike `hook.skip`,
    /// which matches by substring and so at least reaches SOMETHING, a severity
    /// key is an exact name: misspell it and it is inert. Nothing in git says
    /// so, which is how a repo ends up believing a check was downgraded when it
    /// was never touched.
    pub fn is_inert(&self) -> bool {
        !all_checks().contains(&self.check.as_str()) || self.level == Level::Unrecognised
    }

    /// The downgrade itself — a check that runs, reports, and does not block.
    ///
    /// Only when this entry is the one git applies: a shadowed `warn` weakens
    /// nothing.
    pub fn weakens(&self) -> bool {
        self.effective && self.level == Level::Warn && all_checks().contains(&self.check.as_str())
    }

    /// Configured, but overridden by a later entry. Not damage — but not
    /// nothing either, which is why it is shown rather than filtered out.
    pub fn shadowed(&self) -> bool {
        !self.effective
    }
}

/// Every `githooks.severity.*` entry visible from `repo`, with where it came
/// from.
///
/// `--get-regexp` rather than a key-by-key sweep: the point is to find keys
/// nobody expected, including ones naming checks that do not exist.
pub fn read(repo: &Path) -> Vec<SeverityOverride> {
    let Ok(out) = Command::new("git")
        .args([
            "config",
            "--show-origin",
            "--get-regexp",
            r"^githooks\.severity\.",
        ])
        .current_dir(repo)
        .output()
    else {
        return Vec::new();
    };
    resolve(repo, parse(&String::from_utf8_lossy(&out.stdout)))
}

/// Turn raw entries into resolved ones by asking which value git applies.
///
/// A MAP, not a mutation. `parse` used to return `SeverityOverride`s whose
/// `effective` field was `true` for everything until this corrected it — a
/// value that was simply wrong for as long as it existed, and observably so:
/// tests calling `parse` directly got entries claiming to be effective without
/// anyone having asked.
///
/// The question is answered by the RUNTIME — the same call the dispatcher makes
/// — rather than by re-deriving precedence here. Config precedence is git's
/// (system, then global, then local, then includes), and a reimplementation
/// would be a second opinion about the one thing this column must not get
/// wrong.
fn resolve(repo: &Path, raws: Vec<RawEntry>) -> Vec<SeverityOverride> {
    let mut applied: BTreeMap<&str, Option<String>> = BTreeMap::new();
    for raw in &raws {
        if !applied.contains_key(raw.check.as_str()) {
            let v = githooks_runtime::git::stdout_in(
                repo,
                &[
                    "config",
                    "--get",
                    &githooks_runtime::registry::severity_key(&raw.check),
                ],
            );
            applied.insert(raw.check.as_str(), v);
        }
    }
    raws.iter()
        .map(|r| SeverityOverride {
            // A duplicate of the winning VALUE is indistinguishable from the
            // winner and is treated as effective too. That errs toward showing
            // a downgrade rather than hiding one, the safe direction here.
            effective: applied.get(r.check.as_str()).and_then(|v| v.as_deref())
                == Some(r.value.as_str()),
            check: r.check.clone(),
            level: Level::of(&r.value),
            value: r.value.clone(),
            scope: r.scope.clone(),
        })
        .collect()
}

/// Split out from `read` so the origins can be exercised without arranging for
/// git to produce them. A repo can be made to write a local key easily and a
/// global one only by touching the developer's real `~/.gitconfig`, so a test
/// driving `read` alone can assert `Local` and nothing else — which a `scope`
/// field hard-coded to `Local` would satisfy just as well.
fn parse(stdout: &str) -> Vec<RawEntry> {
    stdout
        .lines()
        .filter_map(|line| {
            // `file:/path/to/config\tgithooks.severity.<check> <value>`
            let (origin, rest) = line.split_once('\t')?;
            let (key, value) = rest.split_once(' ')?;
            let check = key.strip_prefix("githooks.severity.")?.trim();
            let value = value.trim();
            if check.is_empty() {
                return None;
            }
            Some(RawEntry {
                check: check.to_string(),
                value: value.to_string(),
                scope: scope_of(origin),
            })
        })
        .collect()
}

/// Build a local entry from raw parts, for fixtures.
#[cfg(test)]
pub fn for_test(check: &str, value: &str) -> SeverityOverride {
    SeverityOverride {
        check: check.to_string(),
        value: value.to_string(),
        level: Level::of(value),
        scope: Scope::Local,
        effective: true,
    }
}

/// A configured entry that git overrides with a later one.
#[cfg(test)]
pub fn shadowed_for_test(check: &str, value: &str) -> SeverityOverride {
    SeverityOverride {
        effective: false,
        ..for_test(check, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_real_check_set_to_warn_is_a_weakening() {
        let e = for_test("pre-commit-clippy", "warn");
        assert!(e.weakens());
        assert!(!e.is_inert());
    }

    /// `block` is the default. Writing it changes nothing, so it must not be
    /// reported as damage — but it is still shown, because it is evidence
    /// somebody was here.
    #[test]
    fn an_explicit_block_weakens_nothing() {
        let e = for_test("pre-commit-clippy", "block");
        assert!(!e.weakens());
        assert!(!e.is_inert());
    }

    /// The two ways to configure nothing at all, which look identical to a
    /// human skimming `.git/config`.
    #[test]
    fn a_misspelt_key_or_value_is_inert() {
        let bad_key = for_test("pre-commit-clipy", "warn");
        assert!(bad_key.is_inert());
        assert!(
            !bad_key.weakens(),
            "a key naming no check cannot have weakened one"
        );

        let bad_value = for_test("pre-commit-clippy", "advisory");
        assert!(bad_value.is_inert());
        assert!(!bad_value.weakens());
    }

    /// `hook.skip` takes substrings; this key does not. A value that would be a
    /// perfectly good skip pattern is an inert severity key, and the dashboard
    /// has to say so rather than quietly resolving it.
    #[test]
    fn a_fragment_is_not_resolved_the_way_a_skip_would_be() {
        assert!(!crate::skips::suppressed_by("clippy").is_empty());
        assert!(for_test("clippy", "warn").is_inert());
    }

    /// A real repo, because the whole module is a parse of `git config
    /// --show-origin --get-regexp` output and a hand-written fixture would only
    /// prove the fixture matches itself. The format — key and value separated
    /// by a space, origin by a tab — is git's, not ours.
    #[test]
    fn reads_what_git_actually_wrote() {
        let d = std::env::temp_dir().join(format!("sev-read-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&d)
                .output()
                .expect("git");
        };
        git(&["init", "-q", "--template=", "."]);
        git(&["config", "githooks.severity.pre-commit-clippy", "warn"]);
        git(&["config", "githooks.severity.pre-commit-prettier", "block"]);
        // An unrelated key under the same section must not be mistaken for one.
        git(&["config", "githooks.other.thing", "warn"]);

        let mut got = read(&d);
        got.sort_by(|a, b| a.check.cmp(&b.check));
        let _ = std::fs::remove_dir_all(&d);

        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got[0].check, "pre-commit-clippy");
        assert_eq!(got[0].level, Level::Warn);
        assert_eq!(got[0].scope, Scope::Local, "written to .git/config");
        assert_eq!(got[1].check, "pre-commit-prettier");
        assert_eq!(got[1].level, Level::Block);
    }

    /// Where a key LIVES is half the answer: deleting a line from `.git/config`
    /// and finding the check still downgraded is the same trap `hook.skip` set,
    /// and `--show-origin` is the only thing that tells them apart.
    #[test]
    fn the_origin_decides_the_scope() {
        let got = parse(concat!(
            "file:/repo/.git/config\tgithooks.severity.pre-commit-clippy warn\n",
            "file:/Users/me/.gitconfig\tgithooks.severity.pre-commit-prettier warn\n",
            "file:/etc/gitconfig\tgithooks.severity.pre-commit-ban-terms warn\n",
        ));
        assert_eq!(got.len(), 3, "{got:?}");
        assert_eq!(got[0].scope, Scope::Local);
        assert_eq!(got[1].scope, Scope::Global);
        assert!(
            matches!(got[2].scope, Scope::Other { .. }),
            "system config is neither of the two the UI can edit: {:?}",
            got[2].scope
        );
    }

    /// End to end against real git, with the precedence that caused the bug.
    ///
    /// Two entries on one key rather than a global/local pair: `--get` returns
    /// the LAST either way, and this needs no `GIT_CONFIG_GLOBAL` — an env var
    /// a test cannot set without racing every other test in the process.
    #[test]
    fn the_entry_git_applies_is_the_one_marked_effective() {
        let d = std::env::temp_dir().join(format!("sev-eff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&d)
                .output()
                .expect("git");
        };
        git(&["init", "-q", "--template=", "."]);
        let key = "githooks.severity.pre-commit-merge-conflict";
        git(&["config", "--add", key, "warn"]);
        git(&["config", "--add", key, "block"]);

        let got = read(&d);
        let _ = std::fs::remove_dir_all(&d);

        assert_eq!(got.len(), 2, "both entries must still be SHOWN: {got:?}");
        let warn = got.iter().find(|e| e.value == "warn").expect("the warn");
        let block = got.iter().find(|e| e.value == "block").expect("the block");
        assert!(!warn.effective, "git applies block, not warn");
        assert!(block.effective);
        assert!(
            !warn.weakens(),
            "the WARN column would report a downgrade the dispatcher does not apply"
        );
    }

    /// No keys at all is the normal case, and it must be an empty list rather
    /// than a parse of git's empty output producing one blank entry.
    #[test]
    fn a_repo_with_no_overrides_reads_empty() {
        let d = std::env::temp_dir().join(format!("sev-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::process::Command::new("git")
            .args(["init", "-q", "--template=", "."])
            .current_dir(&d)
            .output()
            .expect("git");
        let got = read(&d);
        let _ = std::fs::remove_dir_all(&d);
        assert!(got.is_empty(), "{got:?}");
    }
}
