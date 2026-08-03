//! Reading configuration — and reading it the way git itself does.
//!
//! Everything this project can be tuned with is a `git config` key, so the
//! honest implementation of that promise is to let git do the parsing.
//! `git config --type=bool` implements git-config(1) by definition: `on`,
//! `yes`, `1`, an empty value, and every capitalisation of each. A hand-rolled
//! `matches!(v, "true" | "1" | "yes")` is our own dialect wearing git's
//! clothes, and it had already drifted — `git config githooks.fix on` looked
//! like it worked and did not.
//!
//! The exit code carries the part that matters most:
//!
//! | exit | means |
//! |---|---|
//! | 0 | the key is set, and stdout is git's normalised value |
//! | 1 | the key is not set anywhere git looked |
//! | 128 | the key is set to something git refuses to parse, and said so on stderr |
//!
//! Collapsing 1 and 128 into "no" is the bug this module exists to prevent: a
//! limit you believe you raised and did not is exactly the silent-config
//! failure that `hook.skip` announcements were introduced for. So a bad value
//! falls back to the shipped default **and says so**, once per key per run.

use crate::git;
use crate::ui::{highlight, warning_sign};
use std::collections::BTreeSet;
use std::ops::RangeInclusive;
use std::sync::{Mutex, OnceLock};

/// What a key said. Three answers, because "unset" is a state this project
/// makes decisions with — `githooks.commit.gitmoji` has four meanings and one
/// of them is absence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value<T> {
    Unset,
    Set(T),
    /// Set to something that could not be read. `why` is git's own diagnostic
    /// where git produced one, and ours where the constraint is ours (a value
    /// outside an allowed set, or outside a range).
    Bad {
        why: String,
    },
}

impl<T> Value<T> {
    pub fn is_set(&self) -> bool {
        matches!(self, Value::Set(_))
    }
}

/// `git config --type=<ty> --get <key>`, with the three exits kept apart.
///
/// Git failing to run at all is reported as `Unset`: this crate's standing
/// posture is that an unanswerable question takes the default rather than
/// blocking a commit.
fn typed(key: &str, ty: &str) -> Value<String> {
    let type_flag = format!("--type={ty}");
    let Some(out) = git::output(&["config", &type_flag, "--get", key]) else {
        return Value::Unset;
    };
    match out.code {
        0 => Value::Set(out.stdout),
        1 => Value::Unset,
        _ => Value::Bad {
            why: first_line(&out.stderr),
        },
    }
}

/// Git's `fatal:` line, without the noise around it. Empty stderr still yields
/// something printable, because a warning that names no cause is a puzzle.
fn first_line(stderr: &str) -> String {
    let line = stderr.lines().next().unwrap_or("").trim();
    let line = line.strip_prefix("fatal: ").unwrap_or(line);
    if line.is_empty() {
        "git could not read the value".to_string()
    } else {
        line.to_string()
    }
}

pub fn boolean(key: &str) -> Value<bool> {
    match typed(key, "bool") {
        Value::Set(v) => match v.as_str() {
            "true" => Value::Set(true),
            "false" => Value::Set(false),
            other => Value::Bad {
                why: format!("git normalised it to {other:?}, which is neither true nor false"),
            },
        },
        Value::Unset => Value::Unset,
        Value::Bad { why } => Value::Bad { why },
    }
}

/// An integer, in git's own spelling — which includes the `k`/`m`/`g` suffixes
/// git accepts, since `--type=int` expands them before we see them.
pub fn integer(key: &str) -> Value<i64> {
    match typed(key, "int") {
        Value::Set(v) => match v.parse::<i64>() {
            Ok(n) => Value::Set(n),
            Err(_) => Value::Bad {
                why: format!("git returned {v:?}, which is not a whole number"),
            },
        },
        Value::Unset => Value::Unset,
        Value::Bad { why } => Value::Bad { why },
    }
}

/// One of a fixed set of words, compared case-insensitively.
///
/// Git has no `--type` for this, so the value is read raw and checked here —
/// which means this is the one reader whose `Bad` message is ours. It names
/// every accepted spelling, because a rejection that does not say what was
/// wanted sends the reader to the documentation for a list we already hold.
pub fn enumerated(key: &str, allowed: &[&'static str]) -> Value<&'static str> {
    let Some(out) = git::output(&["config", "--get", key]) else {
        return Value::Unset;
    };
    match out.code {
        0 => {
            let got = out.stdout.trim().to_ascii_lowercase();
            match allowed.iter().find(|a| a.eq_ignore_ascii_case(&got)) {
                Some(hit) => Value::Set(hit),
                None => Value::Bad {
                    why: format!("{got:?} is not one of {}", allowed.join(", ")),
                },
            }
        }
        1 => Value::Unset,
        _ => Value::Bad {
            why: first_line(&out.stderr),
        },
    }
}

/// Say once, per key, that a configured value could not be used.
///
/// Deduplicated because a key read twice in one run is a detail of how the
/// code is arranged, and repeating the warning would make it look like two
/// separate mistakes.
pub fn complain(key: &str, why: &str, using: &str) {
    static SAID: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    let said = SAID.get_or_init(|| Mutex::new(BTreeSet::new()));
    // A poisoned mutex means another thread panicked mid-insert; warning twice
    // is strictly better than joining it in panicking.
    let fresh = match said.lock() {
        Ok(mut set) => set.insert(key.to_string()),
        Err(_) => true,
    };
    if fresh {
        eprintln!(
            "{} {}: {why} — using {using}",
            warning_sign().trim(),
            highlight(key)
        );
    }
}

pub fn boolean_or(key: &str, default: bool) -> bool {
    match boolean(key) {
        Value::Set(v) => v,
        Value::Unset => default,
        Value::Bad { why } => {
            complain(key, &why, &default.to_string());
            default
        }
    }
}

/// An integer, clamped to what the setting can actually mean.
///
/// Out of range is treated exactly as unparseable: a `subjectMax` of 0 would
/// block every commit forever from a config file, and one of 10_000 is not a
/// limit. Both are mistakes, and both get the default plus a line saying so.
pub fn integer_or(key: &str, default: i64, range: RangeInclusive<i64>) -> i64 {
    match integer(key) {
        Value::Set(v) if range.contains(&v) => v,
        Value::Set(v) => {
            complain(
                key,
                &format!("{v} is outside {}..={}", range.start(), range.end()),
                &default.to_string(),
            );
            default
        }
        Value::Unset => default,
        Value::Bad { why } => {
            complain(key, &why, &default.to_string());
            default
        }
    }
}

pub fn enumerated_or(key: &str, allowed: &[&'static str], default: &'static str) -> &'static str {
    match enumerated(key, allowed) {
        Value::Set(v) => v,
        Value::Unset => default,
        Value::Bad { why } => {
            complain(key, &why, default);
            default
        }
    }
}

/// Which keys under `prefix` are set at all — one git call for the whole
/// family.
///
/// This runs on the commit path, where four independent `--get` calls would be
/// four processes spent discovering that nobody has configured anything. One
/// `--get-regexp` answers that, and only the keys it names are read for real.
/// Same shape as `registry::Overrides::read`, for the same reason.
///
/// **Names come back lowercased.** Git lowercases the section and key parts of
/// every name it prints, so `githooks.commit.subjectMax` is reported as
/// `githooks.commit.subjectmax`. Keys are case-insensitive on lookup, so this
/// only affects comparison here — hence [`is_present`] rather than a bare
/// `contains`.
pub fn present(prefix: &str) -> BTreeSet<String> {
    let pattern = format!("^{}", regex_escape(prefix));
    let Some(out) = git::output(&["config", "--get-regexp", &pattern]) else {
        return BTreeSet::new();
    };
    if out.code != 0 {
        return BTreeSet::new();
    }
    out.stdout
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .map(|k| k.to_ascii_lowercase())
        .collect()
}

/// Is `key` among the names [`present`] returned? Case-insensitive, because
/// git config key names are.
pub fn is_present(names: &BTreeSet<String>, key: &str) -> bool {
    names.contains(&key.to_ascii_lowercase())
}

/// Escape the characters a config key can hold that a POSIX basic regex would
/// otherwise read as syntax. Only `.` occurs in practice; the rest are here so
/// this cannot become wrong if a caller passes something else.
fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        if matches!(c, '.' | '*' | '[' | ']' | '^' | '$' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Where a key's value came from, for the commands whose job is reading
/// configuration back.
///
/// This costs a second git call per key and must never be used on the commit
/// path — `githooks list` and `githooks setup` are the only callers, and they
/// are already asking git several questions to render one screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Default,
    Local,
    Global,
    System,
    CommandLine,
    Other,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Default => "default",
            Scope::Local => "local",
            Scope::Global => "global",
            Scope::System => "system",
            Scope::CommandLine => "command line",
            Scope::Other => "other",
        }
    }
}

/// `git config --show-origin --get <key>` → which file it came from.
///
/// `Default` for a key nobody set, which is also the answer when git cannot be
/// asked — the value in use is the shipped one either way.
pub fn scope_of(key: &str) -> Scope {
    let Some(out) = git::output(&["config", "--show-origin", "--get", key]) else {
        return Scope::Default;
    };
    if out.code != 0 {
        return Scope::Default;
    }
    // `<origin>\t<value>`; the origin is `file:/path`, `command line:` or
    // `blob:…`, and the path is what tells local from global.
    let origin = out.stdout.split('\t').next().unwrap_or("");
    if origin.starts_with("command line") {
        return Scope::CommandLine;
    }
    let Some(path) = origin.strip_prefix("file:") else {
        return Scope::Other;
    };
    let path = path.trim();
    // A repository's own config is the only one inside a `.git` directory;
    // asking git for the paths rather than guessing at `~` keeps this correct
    // under `GIT_CONFIG_GLOBAL`, worktrees and `$XDG_CONFIG_HOME`.
    if same_file(
        path,
        git::stdout(&["rev-parse", "--git-path", "config"]).as_deref(),
    ) {
        return Scope::Local;
    }
    for (flag, scope) in [("--global", Scope::Global), ("--system", Scope::System)] {
        let listed = git::output(&["config", flag, "--list", "--show-origin"]);
        if let Some(o) = listed {
            if o.code == 0
                && o.stdout
                    .lines()
                    .filter_map(|l| l.split('\t').next())
                    .filter_map(|o| o.strip_prefix("file:"))
                    .any(|p| same_file(path, Some(p.trim())))
            {
                return scope;
            }
        }
    }
    Scope::Other
}

/// Compare two paths as the same file where the filesystem can say so, falling
/// back to the strings. `--git-path` answers relatively (`.git/config`) while
/// `--show-origin` may answer absolutely, so a string comparison alone
/// misreports a local key as `other`.
fn same_file(a: &str, b: Option<&str>) -> bool {
    let Some(b) = b else { return false };
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The distinction the whole module exists for: git's two failure exits
    /// mean opposite things, and `first_line` is what a reader is shown for
    /// the one that is a mistake.
    #[test]
    fn a_fatal_line_is_reported_without_its_prefix() {
        assert_eq!(
            first_line("fatal: bad numeric config value 'wide' for 'a.b'"),
            "bad numeric config value 'wide' for 'a.b'"
        );
        assert_eq!(first_line("first\nsecond"), "first");
    }

    /// A warning that names no cause is a puzzle, so there is always a cause.
    #[test]
    fn an_empty_diagnostic_still_says_something() {
        assert!(!first_line("").is_empty());
        assert!(!first_line("   \n  ").is_empty());
    }

    #[test]
    fn a_key_name_is_escaped_before_it_becomes_a_pattern() {
        // Without escaping, `.` matches any character and the prefix would
        // also select `githooksXcommit.*`.
        assert_eq!(regex_escape("githooks.commit."), "githooks\\.commit\\.");
        assert_eq!(regex_escape("plain"), "plain");
    }

    /// Git lowercases the names it prints, so a presence test that respected
    /// case would answer "not set" for every camelCase key we ship.
    #[test]
    fn presence_is_case_insensitive_because_git_lowercases_names() {
        let names: BTreeSet<String> = ["githooks.commit.subjectmax".to_string()]
            .into_iter()
            .collect();
        assert!(is_present(&names, "githooks.commit.subjectMax"));
        assert!(is_present(&names, "GITHOOKS.COMMIT.SUBJECTMAX"));
        assert!(!is_present(&names, "githooks.commit.bodyWrap"));
    }

    #[test]
    fn every_scope_has_a_name() {
        for s in [
            Scope::Default,
            Scope::Local,
            Scope::Global,
            Scope::System,
            Scope::CommandLine,
            Scope::Other,
        ] {
            assert!(!s.as_str().is_empty());
        }
    }
}
