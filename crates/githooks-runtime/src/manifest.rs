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

/// Why a line could not be used.
///
/// A type rather than a `String`: the prose belongs in `Display`, and a caller
/// that wants to ask "was this a duplicate?" should not have to grep for the
/// word. The tests used to assert on substrings, which coupled them to wording
/// and would have kept passing if the wording stayed while the meaning changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    MissingFields,
    MissingName,
    /// Names a check compiled into the binary.
    NameTaken(String),
    /// A second USABLE line claiming a name already claimed.
    Duplicate(String),
    BadStage(String),
    BadScope(String),
    BadSeverity(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::MissingFields => {
                write!(f, "expected 5 fields: stage name scope severity command")
            }
            ParseError::MissingName => write!(f, "missing name"),
            ParseError::NameTaken(n) => write!(f, "{n:?} is a built-in check name"),
            ParseError::Duplicate(n) => write!(f, "{n:?} is declared twice"),
            ParseError::BadStage(t) => {
                write!(f, "stage {t:?} must be `pre-commit` or `pre-push`")
            }
            ParseError::BadScope(t) => write!(f, "scope {t:?} must be `*` or `*.<ext>`"),
            ParseError::BadSeverity(t) => {
                write!(f, "severity {t:?} must be `block` or `warn`")
            }
        }
    }
}

/// A line that parsed. Every field means something.
///
/// `program` and `args` rather than one `argv`: a runnable check must have a
/// command, and splitting the head off makes that structural instead of a
/// `split_first` guard that can only ever be dead code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    pub name: String,
    pub stage: Stage,
    pub severity: Severity,
    /// Extensions that gate it. Empty means any change — the `*` scope.
    pub exts: Vec<String>,
    pub program: String,
    pub args: Vec<String>,
}

impl Declared {
    /// The command as written, for display.
    pub fn command(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// One manifest line: usable, or not.
///
/// A SUM, not a struct with an `Option<why>` beside the fields. The struct
/// form let a broken line carry a severity, a scope and an argv that meant
/// nothing — and it produced a wrong diagnosis: because broken and usable
/// entries shared one list, a valid line was rejected as "declared twice" for
/// colliding with a line that could not run. Dedup now sees only `Usable`.
///
/// Separate from `External` because the fleet reads ninety-six manifests and may
/// re-read them on every refresh, while `External` holds a `Scope` whose
/// `&'static` slices are LEAKED.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Usable(Declared),
    Broken {
        /// The declared name, or `<file>:<lineno>` when the line has none — a
        /// gap has to be nameable to be reportable.
        name: String,
        /// Broken lines land on pre-commit unless the stage token parsed: seen
        /// on every commit beats seen on every push.
        stage: Stage,
        lineno: usize,
        why: ParseError,
    },
}

impl Line {
    pub fn name(&self) -> &str {
        match self {
            Line::Usable(d) => &d.name,
            Line::Broken { name, .. } => name,
        }
    }
    pub fn stage(&self) -> Stage {
        match self {
            Line::Usable(d) => d.stage,
            Line::Broken { stage, .. } => *stage,
        }
    }
    /// `Some(reason)` when this line declares a check that cannot run.
    pub fn broken(&self) -> Option<String> {
        match self {
            Line::Usable(_) => None,
            Line::Broken { lineno, why, .. } => Some(format!("line {lineno}: {why}")),
        }
    }

    /// Consume into the identity every line has, and either the declaration or
    /// the reason there is none.
    ///
    /// Both consumers — `External::from` and the fleet's projection — used to
    /// destructure this by hand, and both carried an arm for a combination the
    /// type forbids, because they computed the reason BEFORE matching. Written
    /// once, that arm has nowhere to appear.
    pub fn into_parts(self) -> (String, Stage, Result<Declared, String>) {
        let name = self.name().to_string();
        let stage = self.stage();
        let parsed = match self {
            Line::Usable(d) => Ok(d),
            Line::Broken { lineno, why, .. } => Err(format!("line {lineno}: {why}")),
        };
        (name, stage, parsed)
    }
}

/// A check a repository declares, rather than one compiled in.
pub struct External {
    pub name: String,
    pub stage: Stage,
    pub kind: Kind,
}

/// The two things an external can be. `Scope` and `Severity` live only on the
/// runnable side, so a broken external cannot carry a severity nobody applies.
pub enum Kind {
    Runnable {
        scope: Scope,
        severity: Severity,
        program: String,
        args: Vec<String>,
    },
    Unusable {
        why: String,
    },
}

impl Check for External {
    fn name(&self) -> &str {
        &self.name
    }
    fn stage(&self) -> Stage {
        self.stage
    }
    /// Derived for an unusable check rather than stored: it never runs, so its
    /// scope is a question with no answer, and computing one here keeps the
    /// DATA from carrying a value that means nothing.
    fn scope(&self) -> Scope {
        match &self.kind {
            Kind::Runnable { scope, .. } => *scope,
            Kind::Unusable { .. } => Scope::ALWAYS,
        }
    }
    fn severity(&self) -> Severity {
        match &self.kind {
            Kind::Runnable { severity, .. } => *severity,
            // Never consulted: an unusable check reports `Unavailable`, which
            // no severity can turn into a block.
            Kind::Unusable { .. } => Severity::Warn,
        }
    }

    fn run(&self, ctx: &Ctx) -> Outcome {
        let (scope, program, args) = match &self.kind {
            Kind::Runnable {
                scope,
                program,
                args,
                ..
            } => (scope, program, args),
            Kind::Unusable { why } => {
                crate::hooks::common::warn(&format!(
                    "{MANIFEST}: {} — {why}",
                    crate::ui::highlight(&self.name)
                ));
                return Outcome::Unavailable;
            }
        };

        // The scope gate lives HERE, unlike a built-in's, which enforces its own
        // in its first three lines. A declared command has no way to know what
        // was staged, so if this did not gate it, `*.sh` would run on every
        // commit and the column would be decoration.
        //
        // Which files to test against depends on the stage: what is staged for
        // a commit, what is being pushed for a push. `*` short-circuits before
        // either is computed, which is the common case.
        if !scope.files.is_empty() {
            let paths = match self.stage {
                Stage::PreCommit => crate::hooks::common::staged_files(&[]),
                Stage::PrePush => crate::pushrefs::changed_files(ctx.push.get()),
            };
            if !scope.matches(&paths) {
                return Outcome::Passed;
            }
        }
        let root = crate::hooks::common::repo_root();
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
fn parse_scope(token: &str) -> Result<Vec<String>, ParseError> {
    if token == "*" {
        return Ok(Vec::new());
    }
    let mut exts = Vec::new();
    for part in token.split(',') {
        let ext = part
            .strip_prefix('*')
            .filter(|ext| ext.starts_with('.'))
            .ok_or_else(|| ParseError::BadScope(part.to_string()))?;
        exts.push(ext.to_string());
    }
    Ok(exts)
}

fn parse_stage(token: &str) -> Option<Stage> {
    match token {
        "pre-commit" => Some(Stage::PreCommit),
        "pre-push" => Some(Stage::PrePush),
        _ => None,
    }
}

/// A name already spoken for. An external must not be able to shadow
/// `pre-push-branch-protect` — nor silently lose to it, which is what a
/// first-match lookup would do without this.
fn name_is_taken(name: &str) -> bool {
    CHECKS.iter().any(|c| c.name == name) || ENTRYPOINTS.iter().any(|(n, _)| *n == name)
}

/// The four leading tokens and the untouched remainder, or `None` when the line
/// does not have them.
///
/// The arity is in the TYPE. Returning a `Vec` made "four or it is malformed"
/// a rule every caller had to remember and none could be checked against.
///
/// NOT `splitn(5, char::is_whitespace)`: that splits at the FIRST whitespace
/// character every time, so a file aligned into columns — which is how the
/// format invites you to write it — yields empty fields for every run of
/// spaces after the first.
fn tokenise(line: &str) -> Option<([&str; 4], &str)> {
    let mut fields: [&str; 4] = [""; 4];
    let mut rest = line;
    for slot in fields.iter_mut() {
        rest = rest.trim_start();
        let i = rest.find(char::is_whitespace)?;
        *slot = &rest[..i];
        rest = &rest[i..];
    }
    let command = rest.trim();
    (!command.is_empty()).then_some((fields, command))
}

pub fn parse_lines(text: &str) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let lineno = i + 1;
        out.push(parse_line(lineno, line, &out));
    }
    out
}

/// One line, given the lines already accepted.
///
/// `earlier` is read ONLY for the duplicate check, and only its `Usable`
/// entries — a line that cannot run does not reserve its name. Before this, a
/// valid declaration was rejected as "declared twice" for colliding with a
/// broken one, which pointed the reader at the wrong line entirely.
fn parse_line(lineno: usize, line: &str, earlier: &[Line]) -> Line {
    let (fields, command) = match tokenise(line) {
        Some(t) => t,
        // No name to report: fall back to the position, which is the only
        // handle a reader has on a line this malformed.
        None => {
            return broken_at(
                lineno,
                name_or_position(tokenise(line).map(|(f, _)| f[1]).unwrap_or(""), lineno),
                None,
                ParseError::MissingFields,
            )
        }
    };
    let [stage_tok, declared, scope_tok, severity_tok] = fields;
    let stage = parse_stage(stage_tok);
    let name = name_or_position(declared, lineno);
    let fail = |why| broken_at(lineno, name.clone(), stage, why);

    if declared.is_empty() {
        return fail(ParseError::MissingName);
    }
    if name_is_taken(declared) {
        return fail(ParseError::NameTaken(declared.to_string()));
    }
    if earlier
        .iter()
        .any(|l| matches!(l, Line::Usable(d) if d.name == declared))
    {
        return fail(ParseError::Duplicate(declared.to_string()));
    }
    let Some(stage) = stage else {
        return fail(ParseError::BadStage(stage_tok.to_string()));
    };
    let exts = match parse_scope(scope_tok) {
        Ok(e) => e,
        Err(why) => return fail(why),
    };
    let Some(severity) = Severity::parse(severity_tok) else {
        return fail(ParseError::BadSeverity(severity_tok.to_string()));
    };
    // `tokenise` guarantees a non-empty command, so the split cannot fail.
    let mut argv = command.split_whitespace().map(str::to_owned);
    let Some(program) = argv.next() else {
        return fail(ParseError::MissingFields);
    };
    Line::Usable(Declared {
        name: declared.to_string(),
        stage,
        severity,
        exts,
        program,
        args: argv.collect(),
    })
}

fn name_or_position(declared: &str, lineno: usize) -> String {
    if declared.is_empty() {
        format!("{MANIFEST}:{lineno}")
    } else {
        declared.to_string()
    }
}

fn broken_at(lineno: usize, name: String, stage: Option<Stage>, why: ParseError) -> Line {
    Line::Broken {
        name,
        stage: stage.unwrap_or(Stage::PreCommit),
        lineno,
        why,
    }
}

impl From<Line> for External {
    fn from(l: Line) -> External {
        let (name, stage, parsed) = l.into_parts();
        let kind = match parsed {
            Ok(d) => Kind::Runnable {
                scope: if d.exts.is_empty() {
                    Scope::ALWAYS
                } else {
                    Scope::files(leak(d.exts))
                },
                severity: d.severity,
                program: d.program,
                args: d.args,
            },
            Err(why) => Kind::Unusable { why },
        };
        External { name, stage, kind }
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
///
/// `pub(crate)`, NOT `pub`. The answer depends on the working directory at the
/// FIRST call and is then cached for the life of the process — safe in a hook,
/// which handles one repository and exits, and a trap for anything that walks
/// many. The fleet crate reads `read_lines(path)` instead, which takes the
/// repository it means.
pub(crate) fn externals() -> &'static [External] {
    static EXTERNALS: OnceLock<Vec<External>> = OnceLock::new();
    EXTERNALS.get_or_init(|| {
        let root = crate::hooks::common::repo_root();
        let root = Path::new(&root);
        let declared = read(root);
        // Untrusted declarations are kept and DISABLED, not dropped. The names
        // stay visible in `githooks list`, in the dashboard and in the "could
        // not run" roll-up, because a repository quietly declaring checks that
        // never run is the failure this project is arranged against — and the
        // reader needs to know there is a decision waiting for them.
        match crate::trust::why(crate::trust::state(root)) {
            None => declared,
            Some(reason) => declared
                .into_iter()
                .map(|external| External {
                    name: external.name,
                    stage: external.stage,
                    kind: Kind::Unusable {
                        why: reason.to_string(),
                    },
                })
                .collect(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(text: &str) -> Line {
        let mut v = parse_lines(text);
        assert_eq!(v.len(), 1, "expected one entry from {text:?}");
        v.pop().expect("one")
    }

    /// The error a line produced, as a VALUE. Tests used to match on the prose,
    /// which coupled them to wording and would have kept passing if the wording
    /// stayed while the meaning changed.
    fn why(l: &Line) -> ParseError {
        match l {
            Line::Broken { why, .. } => why.clone(),
            Line::Usable(d) => panic!("{} parsed when it should not have", d.name),
        }
    }

    fn usable(l: &Line) -> &Declared {
        match l {
            Line::Usable(d) => d,
            Line::Broken { name, why, .. } => panic!("{name} failed to parse: {why}"),
        }
    }

    #[test]
    fn parses_the_documented_example() {
        let v = parse_lines(
            "# stage       name        scope   severity  command\n\
             pre-commit    shellcheck  *.sh    block     scripts/lint-shell.sh\n\
             pre-push      smoke       *       warn      make smoke\n",
        );
        assert_eq!(v.len(), 2);

        let a = usable(&v[0]);
        assert_eq!(a.name, "shellcheck");
        assert_eq!(a.stage, Stage::PreCommit);
        assert_eq!(a.severity, Severity::Block);
        assert_eq!(a.program, "scripts/lint-shell.sh");
        assert!(a.args.is_empty());
        assert_eq!(a.exts, [".sh"]);

        let b = usable(&v[1]);
        assert_eq!(b.stage, Stage::PrePush);
        assert_eq!(b.severity, Severity::Warn);
        // A command with arguments is split, not handed to a shell.
        assert_eq!(b.program, "make");
        assert_eq!(b.args, ["smoke"]);
        assert!(b.exts.is_empty(), "`*` gates on nothing");
    }

    /// Blank lines and comments are not entries, and must not become broken
    /// ones — a file that is mostly documentation would otherwise report a
    /// dozen gaps.
    #[test]
    fn comments_and_blank_lines_produce_nothing() {
        assert!(parse_lines("\n  \n# just a comment\n\t# indented\n").is_empty());
    }

    /// The rule the module commits to: a line that cannot be understood still
    /// yields a check, so its absence is visible. Matched by VARIANT.
    #[test]
    fn a_malformed_line_becomes_a_visible_gap() {
        let cases: [(&str, ParseError); 4] = [
            (
                "pre-commit shellcheck *.sh block\n",
                ParseError::MissingFields,
            ),
            (
                "nonsense shellcheck *.sh block x\n",
                ParseError::BadStage("nonsense".into()),
            ),
            (
                "pre-commit shellcheck ?.sh block x\n",
                ParseError::BadScope("?.sh".into()),
            ),
            (
                "pre-commit shellcheck *.sh loud x\n",
                ParseError::BadSeverity("loud".into()),
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(why(&one(text)), expected, "for {text:?}");
        }
    }

    /// The prose still has to locate the line, even though the tests no longer
    /// depend on its wording.
    #[test]
    fn a_gap_reports_where_it_is() {
        let l = one("pre-commit shellcheck *.sh loud x\n");
        let said = l.broken().expect("broken");
        assert!(said.contains("line 1"), "{said}");
        assert!(said.contains("severity"), "{said}");
    }

    /// A gap with no name cannot be reported, and a line this broken has none.
    #[test]
    fn a_nameless_line_is_named_after_its_position() {
        let l = one("pre-commit\n");
        assert_eq!(l.name(), ".githooks.conf:1");
        assert_eq!(why(&l), ParseError::MissingFields);
    }

    /// An external must not be able to take a built-in's name — it would either
    /// shadow `pre-push-branch-protect` or silently lose to it, and neither is
    /// something a repository should be able to do by editing a text file.
    #[test]
    fn a_built_in_name_is_refused() {
        assert_eq!(
            why(&one("pre-commit pre-commit-clippy *.rs block x\n")),
            ParseError::NameTaken("pre-commit-clippy".into())
        );
        // Including the four names git itself invokes.
        assert_eq!(
            why(&one("pre-commit pre-push * block x\n")),
            ParseError::NameTaken("pre-push".into())
        );
    }

    /// Two USABLE lines with one name: the second cannot be addressed by
    /// `hook.skip` or by a severity override, so it is refused.
    #[test]
    fn a_duplicate_name_is_refused() {
        let v = parse_lines(
            "pre-commit smoke * block a\n\
             pre-push   smoke * block b\n",
        );
        assert_eq!(v.len(), 2);
        assert_eq!(usable(&v[0]).name, "smoke");
        assert_eq!(why(&v[1]), ParseError::Duplicate("smoke".into()));
    }

    /// A line that cannot run does not RESERVE its name.
    ///
    /// It used to: broken and usable entries shared one list, so a valid
    /// declaration was rejected as "declared twice" for colliding with a line
    /// that could never execute — pointing the reader at the wrong line, and
    /// forcing them to fix the first before the second would work at all.
    #[test]
    fn a_broken_line_does_not_reserve_its_name() {
        let v = parse_lines(
            "pre-commit smoke * LOUD   make a\n\
             pre-commit smoke * block  make b\n",
        );
        assert_eq!(v.len(), 2);
        assert_eq!(why(&v[0]), ParseError::BadSeverity("LOUD".into()));
        let good = usable(&v[1]);
        assert_eq!(good.name, "smoke");
        assert_eq!(good.program, "make");
    }

    /// Alignment is cosmetic. A file someone has lined up with tabs, or not
    /// lined up at all, must parse identically.
    #[test]
    fn field_alignment_does_not_matter() {
        let spaced = one("pre-commit      shellcheck    *.sh      block     make lint\n");
        let tabbed = one("pre-commit\tshellcheck\t*.sh\tblock\tmake lint\n");
        assert_eq!(usable(&spaced), usable(&tabbed));
        assert_eq!(usable(&spaced).args, ["lint"]);
    }

    #[test]
    fn several_extensions_can_gate_one_check() {
        let e = External::from(one("pre-commit shell *.sh,*.bash block make lint\n"));
        assert!(e.scope().matches(&["a.bash".into()]));
        assert!(e.scope().matches(&["a.sh".into()]));
        assert!(!e.scope().matches(&["a.zsh".into()]));
    }

    /// `tokenise` states its arity in the type, so "four tokens then a command"
    /// is checked rather than remembered.
    #[test]
    fn tokenise_wants_four_fields_and_a_command() {
        assert!(tokenise("a b c").is_none(), "too few fields");
        assert!(tokenise("a b c d").is_none(), "four fields, no command");
        // Trailing whitespace reaches the four fields but still leaves nothing
        // to run — the case the `?` on the last field cannot catch.
        assert!(
            tokenise("a b c d   ").is_none(),
            "command is all whitespace"
        );
        assert!(tokenise("a b c d\t").is_none(), "command is a tab");
        let (fields, cmd) = tokenise("a  b\tc   d   run it").expect("four and a command");
        assert_eq!(fields, ["a", "b", "c", "d"]);
        assert_eq!(cmd, "run it");
    }

    /// An unusable line carries no command at all — the type has nowhere to put
    /// one, which is the point of the split.
    #[test]
    fn an_unusable_external_holds_no_command() {
        let e = External::from(one("pre-commit shellcheck *.sh loud echo hi\n"));
        assert!(matches!(e.kind, Kind::Unusable { .. }));
        // And it can never block, whatever severity anyone configures.
        assert_eq!(e.severity(), Severity::Warn);
    }

    /// A missing manifest is the normal case and must not be an error.
    #[test]
    fn a_repository_with_no_manifest_declares_nothing() {
        assert!(read(Path::new("/nonexistent-c8f2")).is_empty());
        assert!(read_lines(Path::new("/nonexistent-c8f2")).is_empty());
    }

    /// `Line` exists to spare the dashboard a leak, not to become a second
    /// opinion about what a manifest says.
    #[test]
    fn the_leaking_and_non_leaking_parsers_agree() {
        let text = "pre-commit  shellcheck  *.sh,*.bash  block  make lint\n\
                    pre-push    smoke       *            warn   make smoke\n\
                    pre-commit  broken      ?            block  x\n";
        let lines = parse_lines(text);
        let externals = parse(text);
        assert_eq!(lines.len(), externals.len());
        for (l, e) in lines.iter().zip(&externals) {
            assert_eq!(l.name(), e.name());
            assert_eq!(l.stage(), e.stage());
            assert_eq!(
                l.broken().is_some(),
                matches!(e.kind, Kind::Unusable { .. })
            );
            if let Line::Usable(d) = l {
                assert_eq!(d.severity, e.severity());
                // The scope the dashboard would DESCRIBE is the scope the
                // dispatcher would ENFORCE.
                assert_eq!(d.exts, e.scope().files);
            }
        }
    }
}
