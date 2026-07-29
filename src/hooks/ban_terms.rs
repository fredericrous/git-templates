//! pre-commit-ban-terms — refuse focused/debug leftovers in staged JS/TS.
//!
//! Two stages, kept exactly as the JS had them:
//!   1. `git diff --cached -G<loose>` picks candidate files cheaply, and keeps
//!      the check scoped to what this commit touches — a pre-existing violation
//!      in an untouched part of an edited file is not this commit's problem.
//!   2. Each candidate is re-checked against its STAGED content with comments
//!      and string literals blanked, which is where correctness lives.
//!
//! Stage 1 stays deliberately loose (POSIX regex, flavour varies by platform);
//! a loose prefilter costs one extra file read, a strict one misses violations.

use crate::git;
use crate::ui::color;

/// (label, loose `git diff -G` prefilter, precise matcher)
struct Term {
    label: &'static str,
    prefilter: &'static str,
    matches: fn(&str) -> bool,
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// `(?<![\w.$])<word>` — not preceded by an identifier char, a dot or a `$`.
/// The dot is what keeps `foo.fit(` out; without it `layout.fit(` is a false
/// positive.
fn preceded_ok(src: &str, at: usize) -> bool {
    src[..at]
        .chars()
        .next_back()
        .map(|c| !(is_ident(c) || c == '.'))
        .unwrap_or(true)
}

/// `<word>\s*\(` — the call form.
fn call_of(src: &str, word: &str) -> bool {
    let mut from = 0;
    while let Some(i) = src[from..].find(word) {
        let at = from + i;
        let after = at + word.len();
        if preceded_ok(src, at) && src[after..].trim_start().starts_with('(') {
            return true;
        }
        from = at + word.len();
    }
    false
}

/// `(?<![\w.$])debugger(?![\w$])` — the bare statement, so `debuggerish` and
/// `x.debugger` both pass.
fn bare_debugger(src: &str) -> bool {
    let word = "debugger";
    let mut from = 0;
    while let Some(i) = src[from..].find(word) {
        let at = from + i;
        let after = at + word.len();
        let next_ok = src[after..]
            .chars()
            .next()
            .map(|c| !is_ident(c))
            .unwrap_or(true);
        if preceded_ok(src, at) && next_ok {
            return true;
        }
        from = at + word.len();
    }
    false
}

/// `(?<![\w$])(describe|context|it)\.(skip|only)(?![\w$])`
///
/// The trailing guard is the whole point: `describe.skipIf(...)` is vitest's
/// legitimate conditional API and must pass, while `describe.skip` must not.
/// Note the LEADING guard here excludes only identifier chars, not `.` — that
/// matches the JS, so `foo.describe.skip` is still caught.
fn focused_suite(src: &str) -> bool {
    for head in ["describe", "context", "it"] {
        for tail in ["skip", "only"] {
            let needle = format!("{head}.{tail}");
            let mut from = 0;
            while let Some(i) = src[from..].find(&needle) {
                let at = from + i;
                let after = at + needle.len();
                let before_ok = src[..at]
                    .chars()
                    .next_back()
                    .map(|c| !is_ident(c))
                    .unwrap_or(true);
                let after_ok = src[after..]
                    .chars()
                    .next()
                    .map(|c| !is_ident(c))
                    .unwrap_or(true);
                if before_ok && after_ok {
                    return true;
                }
                from = at + needle.len();
            }
        }
    }
    false
}

const TERMS: [Term; 4] = [
    Term {
        label: "fit",
        prefilter: r"\s*fit\(",
        matches: |s| call_of(s, "fit"),
    },
    Term {
        label: "fdescribe",
        prefilter: r"\s*fdescribe\(",
        matches: |s| call_of(s, "fdescribe"),
    },
    Term {
        label: "debugger",
        prefilter: "debugger;?",
        matches: bare_debugger,
    },
    Term {
        label: "skipOnly",
        prefilter: r"(describe|context|it)\.(skip|only)",
        matches: focused_suite,
    },
];

#[derive(Clone, Copy, PartialEq)]
enum S {
    Code,
    Line,
    Block,
    Single,
    Double,
    Template,
}

/// Blank comments and the insides of string/template literals, preserving
/// length and line count so offsets still line up — blanked, not deleted, for
/// that reason.
///
/// NOT a parser. A regex literal containing an escaped slash can be misread as
/// a comment opener, which OVER-blanks — costing a missed warning rather than a
/// false alarm. That is the right way round for something standing between a
/// person and their commit, and it is why this stays a faithful port rather
/// than a rewrite (see the PR: a stricter tokenizer is a separate change,
/// because it can newly BLOCK commits).
pub fn blank_non_code(src: &str) -> String {
    let b: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut state = S::Code;
    let mut i = 0;
    let keep = |c: char| if c == '\n' { '\n' } else { ' ' };

    while i < b.len() {
        let ch = b[i];
        let next = b.get(i + 1).copied();
        match state {
            S::Code => {
                if ch == '/' && next == Some('/') {
                    state = S::Line;
                    out.push_str("  ");
                    i += 2;
                } else if ch == '/' && next == Some('*') {
                    state = S::Block;
                    out.push_str("  ");
                    i += 2;
                } else if ch == '\'' || ch == '"' || ch == '`' {
                    state = match ch {
                        '\'' => S::Single,
                        '"' => S::Double,
                        _ => S::Template,
                    };
                    out.push(ch);
                    i += 1;
                } else {
                    out.push(ch);
                    i += 1;
                }
            }
            S::Line => {
                if ch == '\n' {
                    state = S::Code;
                    out.push(ch);
                } else {
                    out.push(' ');
                }
                i += 1;
            }
            S::Block => {
                if ch == '*' && next == Some('/') {
                    state = S::Code;
                    out.push_str("  ");
                    i += 2;
                } else {
                    out.push(keep(ch));
                    i += 1;
                }
            }
            _ => {
                if ch == '\\' {
                    // Blank the escape AND what it escapes, so \" never closes.
                    out.push_str(if next.is_none() { " " } else { "  " });
                    i += 2;
                    continue;
                }
                let closes = matches!(
                    (state, ch),
                    (S::Single, '\'') | (S::Double, '"') | (S::Template, '`')
                );
                if closes {
                    state = S::Code;
                    out.push(ch);
                } else {
                    out.push(keep(ch));
                }
                i += 1;
            }
        }
    }
    out
}

fn is_searchable(file: &str) -> bool {
    let f = file.rsplit('/').next().unwrap_or(file);
    [".js", ".jsx", ".ts", ".tsx", ".vue"]
        .iter()
        .any(|e| f.ends_with(e))
}

pub fn run(hook_name: &str, _args: &[std::ffi::OsString]) -> i32 {
    // This file necessarily NAMES every term it bans, so it must never flag
    // itself. Compare on the file STEM against the hook name we were invoked
    // as: the hook is checked from two layouts — installed at .git/hooks/<name>
    // and as source at templates/hooks/<name> — and a path-relative comparison
    // never matched from the second, which once made this very file
    // uncommittable.
    //
    // NOT argv[0]: that is now the `githooks` binary, so deriving the name from
    // it excluded nothing and the hook flagged its own source. Caught by the
    // existing suite.
    let stem_matches_self = |file: &str| {
        let base = file.rsplit('/').next().unwrap_or(file);
        let stem = base.split_once('.').map(|(s, _)| s).unwrap_or(base);
        stem == hook_name
    };

    let mut status = 0;
    for term in &TERMS {
        let arg = format!("-G{}", term.prefilter);
        let Some(out) = git::stdout(&["diff", "--cached", &arg, "--diff-filter=d", "--name-only"])
        else {
            continue;
        };
        let matches: Vec<&str> = out
            .lines()
            .map(str::trim)
            .filter(|f| !f.is_empty())
            .filter(|f| is_searchable(f))
            .filter(|f| !stem_matches_self(f))
            .filter(|f| {
                match git::stdout(&["show", &format!(":{f}")]) {
                    // Unreadable (binary, or vanished between the two git
                    // calls): keep the prefilter's verdict rather than
                    // silently clearing it.
                    None => true,
                    Some(content) => (term.matches)(&blank_non_code(&content)),
                }
            })
            .collect();

        if !matches.is_empty() {
            if status == 0 {
                eprintln!("  {} Unwanted terms found", color("\u{2717}", "160"));
            }
            status = 1;
            println!(
                "    The following files contains '{}' in them:",
                color(term.label, "208")
            );
            for m in matches {
                println!("    - {}", color(m, "208"));
            }
        }
    }
    if status == 0 {
        println!(
            "  {} No unwanted terms where found",
            color("\u{2713}", "112")
        );
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catches_the_banned_forms() {
        assert!(call_of("fit('x', () => {})", "fit"));
        assert!(call_of("  fit (", "fit"));
        assert!(call_of("fdescribe('x')", "fdescribe"));
        assert!(bare_debugger("  debugger;"));
        assert!(bare_debugger("debugger"));
        assert!(focused_suite("describe.skip('x')"));
        assert!(focused_suite("it.only('x')"));
        assert!(focused_suite("context.skip('x')"));
    }

    /// The false positives that forced the two-stage design.
    #[test]
    fn leaves_lookalikes_alone() {
        assert!(!call_of("profit(", "fit")); // preceded by a word char
        assert!(!call_of("layout.fit(", "fit")); // preceded by a dot
        assert!(!bare_debugger("debuggerish")); // trailing guard
        assert!(!bare_debugger("x.debugger")); // preceded by a dot
        assert!(!focused_suite("describe.skipIf(cond)")); // vitest's real API
        assert!(!focused_suite("it.onlyWhen(x)"));
    }

    #[test]
    fn blanks_comments_and_strings_keeping_layout() {
        let src = "a\n// debugger;\nb";
        let out = blank_non_code(src);
        assert_eq!(out.len(), src.len(), "length must be preserved");
        assert_eq!(out.lines().count(), src.lines().count());
        assert!(!bare_debugger(&out), "a term in a comment is discussion");

        assert!(!bare_debugger(&blank_non_code("const s = 'debugger';")));
        assert!(!bare_debugger(&blank_non_code("const s = `debugger`;")));
        assert!(!call_of(&blank_non_code("/* fit( */"), "fit"));
    }

    #[test]
    fn an_escape_never_closes_a_string() {
        // If \" closed the run, the trailing code would be scanned as code.
        let out = blank_non_code(r#"const s = "a\"b"; debugger;"#);
        assert!(
            bare_debugger(&out),
            "real code after the string must survive"
        );
        assert!(!call_of(&blank_non_code(r#"const s = "a\"fit(";"#), "fit"));
    }

    #[test]
    fn only_js_like_files_are_searched() {
        for f in ["a.js", "a.jsx", "a.ts", "a.tsx", "a.vue", "dir/b.ts"] {
            assert!(is_searchable(f), "{f}");
        }
        for f in ["a.rs", "a.md", "a.json", "README"] {
            assert!(!is_searchable(f), "{f}");
        }
    }
}
