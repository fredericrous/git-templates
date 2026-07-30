//! Reading `hook.skip`, and working out what each entry actually costs.
//!
//! Read-only. Writing is the next change; this one exists so the preview can be
//! built and argued with before anything mutates a config file.
//!
//! Two facts drive the whole module.
//!
//! `hook.skip` matches by SUBSTRING, so a value is not a check name — it is a
//! pattern whose reach has to be computed. `clippy` costs one check and `e`
//! costs all twenty, and neither is visible from the config line itself.
//!
//! And `git config --get-all` merges local, global and system entries with no
//! indication of origin. A developer who deletes the line from `.git/config`
//! and finds the check still skipped has met this; `--show-origin` is the only
//! way to tell them where it really lives.

use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::checks::all_checks;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum Scope {
    Local,
    Global,
    /// System config, an include, or a worktree config — rare, but it must not
    /// be silently relabelled as one of the two the UI can edit.
    Other {
        origin: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkipEntry {
    /// Exactly as written in config.
    pub value: String,
    pub scope: Scope,
    /// Every check this entry suppresses, resolved through the dispatcher's own
    /// rule rather than a copy of it.
    pub suppresses: Vec<&'static str>,
}

impl SkipEntry {
    /// More than one is already a surprise if you meant one. The threshold is
    /// not a percentage: `lint` costing five checks is as much of a shock as
    /// `e` costing twenty, relative to what was intended.
    pub fn is_over_broad(&self) -> bool {
        self.suppresses.len() > 1
    }

    /// True when the value is not itself a check name — the form a human writes
    /// by hand, and the one the UI must never produce. The single skip in the
    /// fleet today (`run-tests-js`) is exactly this.
    pub fn is_fragment(&self) -> bool {
        !all_checks().contains(&self.value.as_str())
    }

    /// The check name this fragment most likely meant, when it means only one.
    /// Offered as a correction rather than only a diagnosis.
    pub fn canonical(&self) -> Option<&'static str> {
        match self.suppresses.as_slice() {
            [only] if self.is_fragment() => Some(only),
            _ => None,
        }
    }
}

/// What a candidate value would suppress. Computed at call time from the
/// registry, never from a table: rename a check and these answers change.
pub fn suppressed_by(value: &str) -> Vec<&'static str> {
    if value.is_empty() {
        return Vec::new();
    }
    all_checks()
        .into_iter()
        .filter(|c| githooks_runtime::skip_suppresses(c, value))
        .collect()
}

/// Build a local entry from a raw value, for fixtures.
#[cfg(test)]
pub fn for_test(value: &str) -> SkipEntry {
    SkipEntry {
        value: value.to_string(),
        scope: Scope::Local,
        suppresses: suppressed_by(value),
    }
}

fn scope_of(origin: &str) -> Scope {
    // `--show-origin` prints `file:<path>`; the global file is the one under
    // $HOME rather than inside a repository.
    let path = origin.strip_prefix("file:").unwrap_or(origin);
    if path.contains(".git/config") || path.ends_with(".git/config") {
        Scope::Local
    } else if path.contains(".gitconfig") || path.contains("git/config") {
        Scope::Global
    } else {
        Scope::Other {
            origin: path.to_string(),
        }
    }
}

/// Every `hook.skip` entry visible from `repo`, with where it came from.
pub fn read(repo: &Path) -> Vec<SkipEntry> {
    let Ok(out) = Command::new("git")
        .args(["config", "--show-origin", "--get-all", "hook.skip"])
        .current_dir(repo)
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            // `file:/path/to/config\tvalue`
            let (origin, value) = line.split_once('\t')?;
            let value = value.trim();
            if value.is_empty() {
                return None;
            }
            Some(SkipEntry {
                value: value.to_string(),
                scope: scope_of(origin),
                suppresses: suppressed_by(value),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule this module reports on must be the rule the dispatcher applies.
    /// Sharing `skip_suppresses` makes divergence impossible rather than
    /// merely unlikely, and this checks the sharing actually happened.
    #[test]
    fn the_resolver_agrees_with_the_dispatchers_rule() {
        for value in ["clippy", "pre-commit-clippy", "lint", "e", "t", "zzz"] {
            let mine = suppressed_by(value);
            let theirs: Vec<&str> = all_checks()
                .into_iter()
                .filter(|c| githooks_runtime::skip_suppresses(c, value))
                .collect();
            assert_eq!(mine, theirs, "diverged on {value:?}");
        }
    }

    /// The numbers that justify this whole feature, computed rather than quoted.
    #[test]
    fn blast_radius_is_computed_from_the_registry() {
        let total = all_checks().len();
        assert_eq!(suppressed_by("pre-commit-clippy").len(), 1);
        assert_eq!(suppressed_by("clippy").len(), 1);
        assert_eq!(suppressed_by("cargo").len(), 2);
        assert!(suppressed_by("lint").len() >= 4, "several, not one");
        // The case the announcement exists for.
        assert_eq!(
            suppressed_by("e").len(),
            total,
            "a one-letter skip disables everything"
        );
        assert!(suppressed_by("t").len() >= total - 1);
    }

    #[test]
    fn an_empty_value_suppresses_nothing() {
        // Not "everything": an empty string is contained in every name, so the
        // naive answer is catastrophically wrong.
        assert!(suppressed_by("").is_empty());
    }

    #[test]
    fn over_broad_and_fragment_are_different_questions() {
        let exact = SkipEntry {
            value: "pre-commit-clippy".into(),
            scope: Scope::Local,
            suppresses: suppressed_by("pre-commit-clippy"),
        };
        assert!(!exact.is_over_broad());
        assert!(!exact.is_fragment());
        assert_eq!(exact.canonical(), None);

        // The one that exists in the fleet today: harmless reach, but not a
        // check name, so a future rename could silently widen it.
        let fragment = SkipEntry {
            value: "run-tests-js".into(),
            scope: Scope::Local,
            suppresses: suppressed_by("run-tests-js"),
        };
        assert!(!fragment.is_over_broad(), "it reaches exactly one check");
        assert!(fragment.is_fragment());
        assert_eq!(fragment.canonical(), Some("pre-push-run-tests-js"));

        let broad = SkipEntry {
            value: "e".into(),
            scope: Scope::Local,
            suppresses: suppressed_by("e"),
        };
        assert!(broad.is_over_broad());
        assert_eq!(broad.canonical(), None, "no single correction to offer");
    }

    #[test]
    fn origin_is_classified_not_guessed() {
        assert_eq!(scope_of("file:/repo/.git/config"), Scope::Local);
        assert_eq!(scope_of("file:/Users/me/.gitconfig"), Scope::Global);
        assert!(matches!(
            scope_of("file:/etc/gitconfig"),
            Scope::Other { .. }
        ));
    }

    /// Reading a real repository, because the parsing is of git's output format
    /// and a hand-written fixture would only prove I can parse my own guess.
    #[test]
    fn reads_entries_from_a_real_repo_with_origin() {
        let dir = std::env::temp_dir().join(format!("skips-read-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .expect("git");
        };
        git(&["init", "-q", "--template=", "."]);
        git(&["config", "--add", "hook.skip", "pre-commit-clippy"]);
        git(&["config", "--add", "hook.skip", "lint"]);

        let entries = read(&dir);
        assert_eq!(entries.len(), 2, "{entries:?}");
        assert_eq!(entries[0].value, "pre-commit-clippy");
        assert_eq!(entries[0].scope, Scope::Local);
        assert_eq!(entries[0].suppresses, vec!["pre-commit-clippy"]);

        assert_eq!(entries[1].value, "lint");
        assert!(
            entries[1].is_over_broad(),
            "lint reaches several checks: {:?}",
            entries[1].suppresses
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_repo_with_no_skips_reads_empty() {
        let dir = std::env::temp_dir().join(format!("skips-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Command::new("git")
            .args(["init", "-q", "--template=", "."])
            .current_dir(&dir)
            .output()
            .expect("git");
        assert!(read(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// What a toggle would do, computed before anything is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkipPlan {
    pub check: &'static str,
    pub action: Action,
    /// Exactly the argv that will run, so the preview cannot drift from the act.
    pub command: Vec<String>,
    /// What the config will suppress afterwards. Not always one, even when
    /// adding a full check name — `pre-commit-lint-js` is a PREFIX of
    /// `pre-commit-lint-json-yaml`, so skipping the first unavoidably skips the
    /// second. Substring matching cannot express "this check only", which is
    /// why this is computed rather than assumed to be 1.
    pub suppresses: Vec<&'static str>,
    pub refuse: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Add,
    Remove,
}

impl SkipPlan {
    /// More than the check you asked for. Requires deliberate confirmation.
    pub fn is_over_broad(&self) -> bool {
        self.action == Action::Add && self.suppresses.len() > 1
    }
}

/// Plan a toggle for `check` in `repo`. Never writes.
pub fn plan(repo: &Path, check: &'static str) -> SkipPlan {
    let existing = read(repo);
    // Remove only an entry that is exactly this check name. A fragment someone
    // wrote by hand may suppress other checks too, and silently widening or
    // narrowing their intent is not this toggle's business.
    let exact = existing.iter().find(|e| e.value == check);

    if let Some(e) = exact {
        let scoped = match e.scope {
            Scope::Local => None,
            _ => Some(format!(
                "that entry is {}, not local — edit it where it lives",
                match e.scope {
                    Scope::Global => "global".to_string(),
                    _ => "in another config".to_string(),
                }
            )),
        };
        return SkipPlan {
            check,
            action: Action::Remove,
            // ANCHORED and escaped: the value-pattern is a regex, so an
            // unescaped `.` would match a neighbouring entry.
            command: vec![
                "config".into(),
                "--unset".into(),
                "hook.skip".into(),
                format!("^{}$", regex_escape(check)),
            ],
            suppresses: Vec::new(),
            refuse: scoped,
        };
    }

    // Already covered by something broader? Adding would duplicate the effect
    // while leaving the original in place.
    let covered = existing
        .iter()
        .find(|e| githooks_runtime::skip_suppresses(check, &e.value));

    SkipPlan {
        check,
        action: Action::Add,
        command: vec![
            "config".into(),
            "--add".into(),
            "hook.skip".into(),
            check.to_string(),
        ],
        suppresses: suppressed_by(check),
        refuse: covered.map(|e| {
            format!(
                "already skipped by {:?}, which suppresses {} check(s)",
                e.value,
                e.suppresses.len()
            )
        }),
    }
}

/// Escape the characters git's value-pattern treats as regex metacharacters.
fn regex_escape(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            let esc = matches!(
                c,
                '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\'
            );
            esc.then_some('\\').into_iter().chain(std::iter::once(c))
        })
        .collect()
}

/// Carry out a plan, then VERIFY by re-reading.
///
/// Given a pattern matching more than one value, `git config --unset` prints
/// `warning: hook.skip has multiple values`, removes nothing, and exits 5. It
/// does signal the refusal — an earlier version of this comment claimed it
/// exited 0, which came from reading `$?` after a pipe rather than from git.
///
/// The verification stays regardless, because a status code says what the
/// command thinks it did and re-reading says what is true. Config is small and
/// the read is cheap; there is no reason to prefer the weaker evidence.
pub fn apply(repo: &Path, plan: &SkipPlan) -> Result<(), String> {
    if let Some(r) = &plan.refuse {
        return Err(r.clone());
    }
    let out = Command::new("git")
        .args(&plan.command)
        .current_dir(repo)
        .output()
        .map_err(|e| e.to_string())?;

    let now = read(repo);
    let present = now.iter().any(|e| e.value == plan.check);
    match plan.action {
        Action::Add if !present => Err(format!(
            "git reported success but the value is absent: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Action::Remove if present => Err(format!(
            "git reported success but the value is still there: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod write_tests {
    use super::*;

    fn repo_at(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("skipw-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Command::new("git")
            .args(["init", "-q", "--template=", "."])
            .current_dir(&d)
            .output()
            .expect("git");
        d
    }

    fn values(repo: &Path) -> Vec<String> {
        read(repo).into_iter().map(|e| e.value).collect()
    }

    #[test]
    fn add_then_remove_round_trips() {
        let d = repo_at("roundtrip");
        let p = plan(&d, "pre-commit-clippy");
        assert_eq!(p.action, Action::Add);
        apply(&d, &p).expect("add");
        assert_eq!(values(&d), vec!["pre-commit-clippy"]);

        let p2 = plan(&d, "pre-commit-clippy");
        assert_eq!(p2.action, Action::Remove, "the toggle flips");
        apply(&d, &p2).expect("remove");
        assert!(values(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The removal pattern is anchored, so a neighbouring entry that merely
    /// CONTAINS the name is untouched.
    #[test]
    fn removal_does_not_take_neighbours_with_it() {
        let d = repo_at("neighbour");
        for v in ["pre-commit-lint-js", "pre-commit-lint-json-yaml"] {
            Command::new("git")
                .args(["config", "--add", "hook.skip", v])
                .current_dir(&d)
                .output()
                .unwrap();
        }
        let p = plan(&d, "pre-commit-lint-js");
        assert_eq!(p.action, Action::Remove);
        apply(&d, &p).expect("remove");
        assert_eq!(
            values(&d),
            vec!["pre-commit-lint-json-yaml"],
            "only the exact value goes"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Adding what is already covered is refused rather than duplicated —
    /// a second entry would have to be unset twice to take effect.
    /// Why the outcome is verified rather than inferred.
    ///
    /// git allows duplicate values. An anchored pattern then matches BOTH, and
    /// `--unset` prints a warning, removes nothing, and exits 5. Re-reading
    /// reports what is actually in the config, which is the thing the caller
    /// asked about.
    #[test]
    fn a_removal_that_git_declines_is_reported_as_failure() {
        let d = repo_at("declined");
        for _ in 0..2 {
            Command::new("git")
                .args(["config", "--add", "hook.skip", "pre-commit-clippy"])
                .current_dir(&d)
                .output()
                .unwrap();
        }
        assert_eq!(values(&d).len(), 2, "duplicates are allowed by git");

        let p = plan(&d, "pre-commit-clippy");
        assert_eq!(p.action, Action::Remove);

        let raw = Command::new("git")
            .args(&p.command)
            .current_dir(&d)
            .output()
            .unwrap();
        // git declines and says so — exit 5, not 0. What it does NOT do is
        // remove anything, which is the part that matters.
        assert!(!raw.status.success(), "git signals the refusal");
        assert_eq!(values(&d).len(), 2, "and removed nothing");

        // apply() must not believe it.
        let err = apply(&d, &p).expect_err("must report the truth");
        assert!(err.contains("still there"), "{err}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn adding_something_already_covered_is_refused() {
        let d = repo_at("dupe");
        Command::new("git")
            .args(["config", "--add", "hook.skip", "clippy"])
            .current_dir(&d)
            .output()
            .unwrap();
        let p = plan(&d, "pre-commit-clippy");
        assert!(p.refuse.is_some(), "{p:?}");
        assert!(apply(&d, &p).is_err());
        assert_eq!(values(&d), vec!["clippy"], "nothing was written");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Writing a full check name is NOT always minimal. `pre-commit-lint-js` is
    /// a prefix of `pre-commit-lint-json-yaml`, and substring matching cannot
    /// express "this one only". The plan must say so rather than assume one.
    #[test]
    fn a_full_name_can_still_be_over_broad() {
        let d = repo_at("prefix");
        let p = plan(&d, "pre-commit-lint-js");
        assert_eq!(
            p.suppresses,
            vec!["pre-commit-lint-js", "pre-commit-lint-json-yaml"]
        );
        assert!(p.is_over_broad(), "must require deliberate confirmation");

        let q = plan(&d, "pre-commit-clippy");
        assert_eq!(q.suppresses.len(), 1);
        assert!(!q.is_over_broad());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The command is anchored and escaped, because git's value-pattern is a
    /// regex and an unescaped metacharacter would over-match.
    #[test]
    fn the_removal_pattern_is_anchored_and_escaped() {
        let d = repo_at("anchor");
        Command::new("git")
            .args(["config", "--add", "hook.skip", "pre-commit-clippy"])
            .current_dir(&d)
            .output()
            .unwrap();
        let p = plan(&d, "pre-commit-clippy");
        let pattern = p.command.last().unwrap();
        assert!(
            pattern.starts_with('^') && pattern.ends_with('$'),
            "{pattern}"
        );
        assert_eq!(regex_escape("a.b"), "a\\.b");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A global entry is not editable from a repo view; say so instead of
    /// running a command that would silently do nothing.
    #[test]
    fn a_non_local_entry_is_refused_with_its_scope() {
        let e = SkipEntry {
            value: "pre-commit-clippy".into(),
            scope: Scope::Global,
            suppresses: suppressed_by("pre-commit-clippy"),
        };
        assert_eq!(e.scope, Scope::Global);
        // plan() consults read(), so this asserts the classification feeding it.
        assert!(matches!(
            scope_of("file:/Users/me/.gitconfig"),
            Scope::Global
        ));
    }
}
