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
