//! pre-commit-ban-terms, ported from its 22-case zsh suite.

mod common;
use common::Repo;

fn check(src: &str) -> bool {
    let r = Repo::new();
    r.stage("f.ts", src);
    r.hook("pre-commit-ban-terms", &[]).passed()
}

#[test]
fn rejects_the_banned_forms() {
    for src in [
        "fdescribe('x', () => {});\n",
        "fit('x', () => {});\n",
        "debugger;\n",
        "describe.skip('x');\n",
        "it.only('x');\n",
        "context.skip('x');\n",
    ] {
        assert!(!check(src), "should have been rejected: {src:?}");
    }
}

/// The false positives that forced the two-stage design. `describe.skipIf` is
/// vitest's real conditional API; `profit(` and `layout.fit(` merely contain
/// the banned identifier.
#[test]
fn accepts_the_lookalikes() {
    for src in [
        "describe('x', () => {});\n",
        "describe.skipIf(cond)('x');\n",
        "it.skipIf(cond)('x');\n",
        "describe.runIf(cond)('x');\n",
        "const x = profit(1);\n",
        "layout.fit(1);\n",
        "const d = debuggerUtils();\n",
    ] {
        assert!(check(src), "should have been accepted: {src:?}");
    }
}

/// A term NAMED in a comment or a string is discussion, not code.
#[test]
fn accepts_terms_in_comments_and_literals() {
    for src in [
        "// debugger;\n",
        "/* fit( */\n",
        "/** jsdoc mentioning debugger */\n",
        "const s = 'debugger';\n",
        "const t = `it.only`;\n",
    ] {
        assert!(check(src), "should have been accepted: {src:?}");
    }
}

#[test]
fn a_comment_does_not_excuse_a_real_violation_on_another_line() {
    assert!(!check("// mentions debugger\nconst x = 1;\ndebugger;\n"));
}

/// The tokenizer added in #29: an escaped slash immediately before the
/// terminator used to read as a line comment, blanking the rest of the line so
/// a real violation after it went unreported.
#[test]
fn a_regex_no_longer_hides_code_after_it() {
    assert!(!check(r"const re = /a\//; debugger;"));
    assert!(!check(r"const re = /\//; debugger;"));
}

/// The dangerous direction: a regex mistaken for division would have its
/// contents scanned and report a violation that does not exist.
#[test]
fn terms_inside_a_regex_are_not_violations() {
    assert!(check(r"const re = /it\.only/;"));
    assert!(check(r"const re = /debugger/;"));
}

#[test]
fn division_is_still_division() {
    assert!(!check("const x = a / b; debugger;"));
}

/// Removing a line containing a banned term is not committing one — `-G`
/// matches removed lines as readily as added ones.
#[test]
fn removing_a_violation_is_not_committing_one() {
    let r = Repo::new();
    r.stage("f.ts", "debugger;\n");
    r.commit("test: seed a violation");
    r.stage("f.ts", "const ok = 1;\n");
    assert!(r.hook("pre-commit-ban-terms", &[]).passed());
}

/// Only JS-ish files are searched.
#[test]
fn other_file_types_are_ignored() {
    let r = Repo::new();
    r.stage("notes.md", "debugger;\n");
    assert!(r.hook("pre-commit-ban-terms", &[]).passed());
}

/// End to end, through stage 1's `git diff -G` prefilter as well as the
/// blanker: a banned call inside a template substitution is real code and must
/// be rejected. This was silently accepted before the tokenizer fix.
#[test]
fn a_banned_call_inside_a_template_substitution_is_rejected() {
    assert!(!check("const s = `${fit(1)}`;\n"), "substitution is code");
    assert!(
        !check("const s = `${`${it.only(1)}`}`;\n"),
        "nested substitution is code"
    );
    assert!(
        !check("const s = `${ {a: 1} && fit(2) }`;\n"),
        "a brace inside the substitution must not end it"
    );
}

/// The other direction, which matters more: template TEXT is still a string,
/// so the stricter blanker must not start alarming on prose.
#[test]
fn template_text_is_still_not_code() {
    for src in [
        "const s = `fit(`;\n",
        "const s = `run debugger;`;\n",
        "const s = `see describe.only in the docs`;\n",
        "const s = `\\${fit(1)}`;\n",
    ] {
        assert!(check(src), "should have been accepted: {src:?}");
    }
}
