//! `.githooks.conf` — checks a repository declares for itself.
//!
//! A third party cannot add a Rust module without rebuilding this binary, so
//! extension means declared commands.
//!
//! The manifest is **committed at the repository root**, and that is the point.
//! `.git/hooks` is not committed, so under the old filename-prefix mechanism a
//! team could never actually share a custom hook — every member had to install
//! it by hand, and nothing told them when it changed. That flaw mattered more
//! than the lexicographic ordering usually cited against prefixes.
//!
//! ```text
//! # stage       name        scope   severity  command
//! pre-commit    shellcheck  *.sh    block     scripts/lint-shell.sh
//! pre-push      smoke       *       warn      make smoke
//! ```
//!
//! Whitespace-delimited, in file order. TOML would be nicer to write and costs a
//! dependency tree that would then run on every commit in ninety-six
//! repositories; for four fields and a command, the twenty lines of parsing win.
//! See `scripts/check-no-deps.sh` for why that trade is the default here.
//!
//! ## No shell
//!
//! The command is split on whitespace and executed directly. There is no shell,
//! so no pipes, no redirection, no globbing and no quoting. Two reasons, and the
//! second is the one that decided it: Windows has no `sh`, and every emulation
//! of one this project has tried has been a source of bugs; and a manifest line
//! that silently gained shell semantics would be a much larger thing to have
//! introduced than it looks. A pipeline belongs in a script the line invokes.
//!
//! ## A line that cannot be understood is not skipped
//!
//! A malformed line means a check the repository asked for is not running, which
//! is precisely the "looks verified, enforced nothing" failure `Outcome` exists
//! to name. So a broken line still produces a check — one that runs to
//! `Unavailable` and says why. It appears in the dispatcher's "could not run"
//! roll-up like any other gap, rather than needing a mechanism of its own.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use crate::check::{Check, Outcome, Scope, Severity, Stage};
use crate::registry::{Ctx, CHECKS, ENTRYPOINTS};

pub const MANIFEST: &str = ".githooks.conf";

/// One manifest line, fully parsed and owning everything it holds.
///
/// Separate from `External` because the fleet dashboard reads ninety-six
/// manifests and may re-read them on every refresh, while `External` holds a
/// `Scope` whose `&'static` slices are LEAKED. One parser, and the leak confined
/// to the hook process, which reads one manifest once and exits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub name: String,
    pub stage: Stage,
    pub severity: Severity,
    /// Extensions that gate it. Empty means any change — the `*` scope.
    pub exts: Vec<String>,
    /// Already split. Empty only when `broken` is set.
    pub argv: Vec<String>,
    /// Why this line could not be used. `Some` makes the check inert but
    /// VISIBLE — see the module docs.
    pub broken: Option<String>,
}

/// A check a repository declares, rather than one compiled in.
pub struct External {
    pub name: String,
    pub stage: Stage,
    pub scope: Scope,
    pub severity: Severity,
    /// Already split. Empty only when `broken` is set.
    pub argv: Vec<String>,
    /// Why this line could not be used. `Some` makes the check inert but
    /// VISIBLE — see the module docs.
    pub broken: Option<String>,
}

impl Check for External {
    fn name(&self) -> &str {
        &self.name
    }
    fn stage(&self) -> Stage {
        self.stage
    }
    fn scope(&self) -> Scope {
        self.scope
    }
    fn severity(&self) -> Severity {
        self.severity
    }

    fn run(&self, ctx: &Ctx) -> Outcome {
        if let Some(why) = &self.broken {
            crate::hooks::common::warn(&format!(
                "{MANIFEST}: {} — {why}",
                crate::ui::highlight(&self.name)
            ));
            return Outcome::Unavailable;
        }
        // The scope gate lives HERE, unlike a built-in's, which enforces its own
        // in its first three lines. A declared command has no way to know what
        // was staged, so if this did not gate it, `*.sh` would run on every
        // commit and the column would be decoration.
        //
        // Which files to test against depends on the stage: what is staged for
        // a commit, what is being pushed for a push. `*` short-circuits before
        // either is computed, which is the common case.
        if !self.scope.files.is_empty() {
            let paths = match self.stage {
                Stage::PreCommit => crate::hooks::common::staged_files(&[]),
                Stage::PrePush => crate::pushrefs::changed_files(ctx.push.get()),
            };
            if !self.scope.matches(&paths) {
                return Outcome::Passed;
            }
        }
        let root = crate::hooks::common::repo_root();
        let Some((program, args)) = self.argv.split_first() else {
            return Outcome::Unavailable;
        };
        let mut cmd = Command::new(program);
        cmd.args(args).current_dir(&root).stdin(Stdio::null());
        crate::hooks::common::strip_git_env(&mut cmd);
        match cmd.status() {
            // A command that could not be STARTED has not judged anything. This
            // is the distinction `Outcome` was added for: reporting a missing
            // `shellcheck` as a lint failure sends someone hunting for a lint
            // error that does not exist.
            Err(e) => {
                crate::hooks::common::warn(&format!(
                    "{MANIFEST}: {} could not run {} — {e}",
                    crate::ui::highlight(&self.name),
                    crate::ui::highlight(program)
                ));
                Outcome::Unavailable
            }
            Ok(s) if s.success() => Outcome::Passed,
            Ok(_) => {
                crate::hooks::common::fail(&format!(
                    "{} failed (output above)",
                    crate::ui::highlight(&self.name)
                ));
                Outcome::Failed
            }
        }
    }
}

/// `Scope` holds `&'static` slices so a built-in can be a `const`. A parsed
/// manifest has neither, so its extension list is leaked.
///
/// This is bounded and deliberate: the manifest is read at most once per
/// process, holds a handful of short strings, and the process is a git hook that
/// exits in milliseconds. The alternative — a lifetime on `Scope` — would
/// propagate through the trait, both dispatchers and the fleet crate to buy back
/// a few hundred bytes that the kernel reclaims moments later.
fn leak(exts: Vec<String>) -> &'static [&'static str] {
    let refs: Vec<&'static str> = exts
        .into_iter()
        .map(|s| &*Box::leak(s.into_boxed_str()))
        .collect();
    Box::leak(refs.into_boxed_slice())
}

/// `*` means any change; `*.sh` or `*.sh,*.bash` gate on extensions.
///
/// No `opt_in` counterpart, because the manifest IS the opt-in: a repository
/// that does not want the check deletes the line.
///
/// Returns owned extensions rather than a `Scope`, so validating a manifest
/// costs nothing permanent. Only `External::from` turns these into the
/// `&'static` form `Scope` requires.
fn parse_scope(token: &str) -> Result<Vec<String>, String> {
    if token == "*" {
        return Ok(Vec::new());
    }
    let mut exts = Vec::new();
    for part in token.split(',') {
        let Some(ext) = part.strip_prefix('*') else {
            return Err(format!("scope {part:?} must be `*` or `*.<ext>`"));
        };
        if ext.is_empty() || !ext.starts_with('.') {
            return Err(format!("scope {part:?} must be `*` or `*.<ext>`"));
        }
        exts.push(ext.to_string());
    }
    Ok(exts)
}

fn parse_stage(token: &str) -> Result<Stage, String> {
    match token {
        "pre-commit" => Ok(Stage::PreCommit),
        "pre-push" => Ok(Stage::PrePush),
        other => Err(format!(
            "stage {other:?} must be `pre-commit` or `pre-push`"
        )),
    }
}

fn parse_severity(token: &str) -> Result<Severity, String> {
    Severity::parse(token).ok_or_else(|| format!("severity {token:?} must be `block` or `warn`"))
}

/// A name already spoken for. An external must not be able to shadow
/// `pre-push-branch-protect` — nor silently lose to it, which is what a
/// first-match lookup would do without this.
fn name_is_taken(name: &str) -> bool {
    CHECKS.iter().any(|c| c.name == name) || ENTRYPOINTS.iter().any(|(n, _)| *n == name)
}

/// The first four whitespace-separated tokens, and the untouched remainder.
fn tokenise(line: &str) -> (Vec<&str>, &str) {
    let mut fields = Vec::new();
    let mut rest = line;
    for _ in 0..4 {
        rest = rest.trim_start();
        match rest.find(char::is_whitespace) {
            Some(i) => {
                fields.push(&rest[..i]);
                rest = &rest[i..];
            }
            None => {
                if !rest.is_empty() {
                    fields.push(rest);
                    rest = "";
                }
                break;
            }
        }
    }
    (fields, rest.trim())
}

pub fn parse_lines(text: &str) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let lineno = i + 1;
        // Four tokens, then everything left is the command.
        //
        // NOT `splitn(5, char::is_whitespace)`: that splits at the FIRST
        // whitespace character every time, so a file aligned into columns —
        // which is how the format invites you to write it — yields empty fields
        // for every run of spaces after the first.
        let (fields, command) = tokenise(line);

        // A name we can print even when the line is too broken to have one:
        // a gap has to be nameable to be reportable.
        let fallback = format!("{MANIFEST}:{lineno}");
        let declared = fields.get(1).copied().unwrap_or("");
        let name = if declared.is_empty() {
            fallback.clone()
        } else {
            declared.to_string()
        };

        let broken = |why: String| Line {
            name: name.clone(),
            // Broken lines land on pre-commit even when the stage token is what
            // failed to parse: seen on every commit beats seen on every push.
            stage: fields
                .first()
                .and_then(|s| parse_stage(s).ok())
                .unwrap_or(Stage::PreCommit),
            severity: Severity::Warn,
            exts: Vec::new(),
            argv: Vec::new(),
            broken: Some(format!("line {lineno}: {why}")),
        };

        if fields.len() < 4 || command.is_empty() {
            out.push(broken(
                "expected 5 fields: stage name scope severity command".to_string(),
            ));
            continue;
        }
        if declared.is_empty() {
            out.push(broken("missing name".to_string()));
            continue;
        }
        if name_is_taken(declared) {
            out.push(broken(format!("{declared:?} is a built-in check name")));
            continue;
        }
        if out.iter().any(|e| e.name == declared) {
            out.push(broken(format!("{declared:?} is declared twice")));
            continue;
        }
        let stage = match parse_stage(fields[0]) {
            Ok(s) => s,
            Err(e) => {
                out.push(broken(e));
                continue;
            }
        };
        let exts = match parse_scope(fields[2]) {
            Ok(s) => s,
            Err(e) => {
                out.push(broken(e));
                continue;
            }
        };
        let severity = match parse_severity(fields[3]) {
            Ok(s) => s,
            Err(e) => {
                out.push(broken(e));
                continue;
            }
        };
        let argv: Vec<String> = command.split_whitespace().map(str::to_owned).collect();
        out.push(Line {
            name: declared.to_string(),
            stage,
            severity,
            exts,
            argv,
            broken: None,
        });
    }
    out
}

impl From<Line> for External {
    fn from(l: Line) -> External {
        External {
            scope: if l.exts.is_empty() {
                Scope::ALWAYS
            } else {
                Scope::files(leak(l.exts))
            },
            name: l.name,
            stage: l.stage,
            severity: l.severity,
            argv: l.argv,
            broken: l.broken,
        }
    }
}

pub fn parse(text: &str) -> Vec<External> {
    parse_lines(text).into_iter().map(External::from).collect()
}

/// The manifest for `root`, or an empty list. Read once per process.
pub fn read(root: &Path) -> Vec<External> {
    std::fs::read_to_string(root.join(MANIFEST))
        .map(|t| parse(&t))
        .unwrap_or_default()
}

/// The same file, without building the `Scope`s — for a reader that inspects
/// many repositories and must not leak once per manifest per refresh.
pub fn read_lines(root: &Path) -> Vec<Line> {
    std::fs::read_to_string(root.join(MANIFEST))
        .map(|t| parse_lines(&t))
        .unwrap_or_default()
}

/// Every external declared by the repository this process is running in.
///
/// A `static OnceLock` rather than a leak: the borrow is genuinely `'static`
/// because the storage is, and it also guarantees the file is read once however
/// many checks ask for it.
pub fn externals() -> &'static [External] {
    static EXTERNALS: OnceLock<Vec<External>> = OnceLock::new();
    EXTERNALS.get_or_init(|| read(Path::new(&crate::hooks::common::repo_root())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(text: &str) -> External {
        let mut v = parse(text);
        assert_eq!(v.len(), 1, "expected one entry from {text:?}");
        v.pop().expect("one")
    }

    #[test]
    fn parses_the_documented_example() {
        let v = parse(
            "# stage       name        scope   severity  command\n\
             pre-commit    shellcheck  *.sh    block     scripts/lint-shell.sh\n\
             pre-push      smoke       *       warn      make smoke\n",
        );
        assert_eq!(v.len(), 2);
        assert!(v.iter().all(|e| e.broken.is_none()), "{:?}", v[0].broken);

        assert_eq!(v[0].name, "shellcheck");
        assert_eq!(v[0].stage, Stage::PreCommit);
        assert_eq!(v[0].severity, Severity::Block);
        assert_eq!(v[0].argv, ["scripts/lint-shell.sh"]);
        assert!(v[0].scope.matches(&["a.sh".into()]));
        assert!(!v[0].scope.matches(&["a.rs".into()]));

        assert_eq!(v[1].stage, Stage::PrePush);
        assert_eq!(v[1].severity, Severity::Warn);
        // A command with arguments is split, not handed to a shell.
        assert_eq!(v[1].argv, ["make", "smoke"]);
        assert!(v[1].scope.matches(&["anything".into()]));
    }

    /// Blank lines and comments are not entries, and must not become broken
    /// ones — a file that is mostly documentation would otherwise report a
    /// dozen gaps.
    #[test]
    fn comments_and_blank_lines_produce_nothing() {
        assert!(parse("\n  \n# just a comment\n\t# indented\n").is_empty());
    }

    /// The rule the module docs commit to: a line that cannot be understood
    /// still yields a check, so its absence is visible.
    #[test]
    fn a_malformed_line_becomes_a_visible_gap() {
        for (text, needle) in [
            ("pre-commit shellcheck *.sh block\n", "5 fields"),
            ("nonsense shellcheck *.sh block x\n", "stage"),
            ("pre-commit shellcheck ?.sh block x\n", "scope"),
            ("pre-commit shellcheck *.sh loud x\n", "severity"),
        ] {
            let e = one(text);
            let why = e.broken.expect("must be reported as broken");
            assert!(why.contains(needle), "{why:?} should mention {needle:?}");
            assert!(why.contains("line 1"), "{why:?} must locate the line");
        }
    }

    /// A gap with no name cannot be reported, and a line this broken has none.
    #[test]
    fn a_nameless_line_is_named_after_its_position() {
        let e = one("pre-commit\n");
        assert_eq!(e.name, ".githooks.conf:1");
        assert!(e.broken.is_some());
    }

    /// Running it must never be an option, so it is never `Passed` either.
    #[test]
    fn a_broken_entry_carries_no_command() {
        let e = one("pre-commit shellcheck *.sh loud echo hi\n");
        assert!(
            e.argv.is_empty(),
            "a line we did not understand has no argv"
        );
    }

    /// An external must not be able to take a built-in's name — it would either
    /// shadow `pre-push-branch-protect` or silently lose to it, and neither is
    /// something a repository should be able to do by editing a text file.
    #[test]
    fn a_built_in_name_is_refused() {
        let e = one("pre-commit pre-commit-clippy *.rs block x\n");
        let why = e.broken.expect("must be refused");
        assert!(why.contains("built-in"), "{why:?}");

        // Including the four names git itself invokes.
        let e = one("pre-commit pre-push * block x\n");
        assert!(e.broken.is_some(), "an entrypoint name is taken too");
    }

    /// Two lines with one name: the second cannot be addressed by `hook.skip`
    /// or by a severity override, so it is refused rather than run anonymously.
    #[test]
    fn a_duplicate_name_is_refused() {
        let v = parse(
            "pre-commit smoke * block a\n\
             pre-push   smoke * block b\n",
        );
        assert_eq!(v.len(), 2);
        assert!(v[0].broken.is_none());
        let why = v[1].broken.clone().expect("the second must be refused");
        assert!(why.contains("twice"), "{why:?}");
    }

    /// Alignment is cosmetic. A file someone has lined up with tabs, or not
    /// lined up at all, must parse identically.
    #[test]
    fn field_alignment_does_not_matter() {
        let spaced = one("pre-commit      shellcheck    *.sh      block     make lint\n");
        let tabbed = one("pre-commit\tshellcheck\t*.sh\tblock\tmake lint\n");
        assert_eq!(spaced.name, tabbed.name);
        assert_eq!(spaced.argv, tabbed.argv);
        assert_eq!(spaced.argv, ["make", "lint"]);
    }

    #[test]
    fn several_extensions_can_gate_one_check() {
        let e = one("pre-commit shell *.sh,*.bash block make lint\n");
        assert!(e.scope.matches(&["a.bash".into()]));
        assert!(e.scope.matches(&["a.sh".into()]));
        assert!(!e.scope.matches(&["a.zsh".into()]));
    }

    /// A missing manifest is the normal case and must not be an error.
    #[test]
    fn a_repository_with_no_manifest_declares_nothing() {
        assert!(read(Path::new("/nonexistent-c8f2")).is_empty());
        assert!(read_lines(Path::new("/nonexistent-c8f2")).is_empty());
    }

    /// The two readers must not be allowed to disagree. `Line` exists to spare
    /// the dashboard a leak, not to become a second opinion about what a
    /// manifest says.
    #[test]
    fn the_leaking_and_non_leaking_parsers_agree() {
        let text = "pre-commit  shellcheck  *.sh,*.bash  block  make lint\n\
                    pre-push    smoke       *            warn   make smoke\n\
                    pre-commit  broken      ?            block  x\n";
        let lines = parse_lines(text);
        let externals = parse(text);
        assert_eq!(lines.len(), externals.len());
        for (l, e) in lines.iter().zip(&externals) {
            assert_eq!(l.name, e.name);
            assert_eq!(l.stage, e.stage);
            assert_eq!(l.severity, e.severity);
            assert_eq!(l.argv, e.argv);
            assert_eq!(l.broken, e.broken);
            // And the scope the dashboard would DESCRIBE is the scope the
            // dispatcher would ENFORCE.
            assert_eq!(l.exts, e.scope.files, "scope diverged for {}", l.name);
        }
    }
}
