//! commit-msg — validate the summary line and reformat the message.
//!
//! Validates: subject present and ≤72; a conventional type prefix; a
//! description; description ≤50. Formats: prepend the type's gitmoji, hard-wrap
//! the body at 72, and group the trailing footers with one blank line before
//! them.
//!
//! Ported from ~190 lines of JS. The one structural simplification is how the
//! optional leading emoji is recognised — see `split_leading_emoji`.

use crate::check::Verdict;
use crate::git;
use crate::ui::{error_sign, highlight, valid_sign};

const MAX_SUMMARY_LINE_SIZE: usize = 72;
const MAX_DESCRIPTION_SIZE: usize = 50;
const MAX_BODY_LINE_SIZE: usize = 72;

use crate::vocabulary::{self, COMMIT_TYPES};

pub struct Subject {
    pub prefix: String,
    pub scope: String,
    pub breaking: String,
    pub description: String,
}

/// Drop full-line comments, as git itself does.
pub fn strip_comments(msg: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in msg.split('\n') {
        if !line.starts_with('#') {
            out.push(line);
        }
    }
    out.join("\n")
}

/// Skip a leading emoji cluster.
///
/// The JS carried a ~2KB hand-maintained list of emoji codepoints, and had
/// already been patched once because it matched only the BASE codepoint —
/// leaving a stray variation selector (U+FE0F) between the emoji and the type,
/// so a perfectly good `⬆️ chore: …` was rejected as having no prefix.
///
/// Inverting the test removes that whole class of bug: a conventional type is
/// ASCII lowercase letters, so skip anything that is NOT ASCII, plus spaces.
/// Strictly more permissive than the codepoint list, and permissive in the
/// harmless direction — the type itself is still required below, so this only
/// decides how much leading decoration gets stripped before re-adding ours.
pub fn split_leading_emoji(subject: &str) -> &str {
    subject.trim_start_matches(|c: char| !c.is_ascii() || c == ' ' || c == '\t')
}

/// `^\s*(emoji)?\s*(type)(\(scope\))?(!)?:\s*(.*)$` over the FIRST line only.
///
/// First line only is load-bearing: the JS once used /ms flags, so `^` matched
/// any line start and a body quoting a conventional commit (a revert citing the
/// commit it undid) was picked up as the subject and rewritten into it.
pub fn parse_subject(subject_line: &str) -> Option<Subject> {
    let rest = split_leading_emoji(subject_line);
    let (prefix, rest) = COMMIT_TYPES
        .iter()
        .map(|t| t.name)
        .find(|t| rest.starts_with(t))
        .map(|t| (t.to_string(), &rest[t.len()..]))?;

    // (\([\w-]+\))?
    let (scope, rest) = if let Some(after) = rest.strip_prefix('(') {
        let end = after.find(')')?;
        let inner = &after[..end];
        if inner.is_empty()
            || !inner
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return None;
        }
        (format!("({inner})"), &after[end + 1..])
    } else {
        (String::new(), rest)
    };

    let (breaking, rest) = match rest.strip_prefix('!') {
        Some(r) => ("!".to_string(), r),
        None => (String::new(), rest),
    };

    let description = rest.strip_prefix(':')?.trim_start_matches(' ').to_string();

    Some(Subject {
        prefix,
        scope,
        breaking,
        description,
    })
}

/// Greedy hard wrap at `width`, breaking on spaces — the JS
/// `(?![^\n]{1,w}$)([^\n]{1,w})\s` replace. A word longer than `width` is left
/// intact rather than split.
pub fn wrap(text: &str, width: usize) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in text.split('\n') {
        if line.chars().count() <= width {
            out.push(line.to_string());
            continue;
        }
        let mut current = String::new();
        for word in line.split(' ') {
            if current.is_empty() {
                current.push_str(word);
            } else if current.chars().count() + 1 + word.chars().count() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                out.push(std::mem::take(&mut current));
                current.push_str(word);
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    out.join("\n")
}

/// A trailing footer line: `Key-Word: value`, `BREAKING CHANGE: …`, `Refs: #1`,
/// or blank.
pub fn is_footer(line: &str) -> bool {
    if line.is_empty() {
        return true;
    }
    if let Some(rest) = line
        .strip_prefix("BREAKING CHANGE:")
        .or_else(|| line.strip_prefix("BREAKING-CHANGE:"))
    {
        return rest.starts_with(' ')
            && rest
                .trim_start()
                .starts_with(|c: char| c.is_alphanumeric() || c == '_');
    }
    // The bare (no-colon) form still needs a SEPARATOR after "Refs" — a
    // space or a '#' — or it also matches prose that merely starts with
    // those five letters glued to a digit, like "Refs42 was the original
    // ticket.", sweeping a body line into the footer group.
    if let Some(rest) = line.strip_prefix("Refs:").or_else(|| {
        line.strip_prefix("Refs")
            .filter(|rest| rest.starts_with(' ') || rest.starts_with('#'))
    }) {
        let r = rest.trim_start_matches(' ');
        let r = r.strip_prefix('#').unwrap_or(r);
        if r.starts_with(|c: char| c.is_ascii_digit()) {
            return true;
        }
    }
    is_hyphenated_key(line)
}

/// `^[\w][\w-]*-[\w-]*\w: \w` — a hyphenated trailer key that STARTS the line.
///
/// The anchor is the whole point. The rule used to be the JS regex
/// `/\w-\w{1,}:\s\w/` applied ANYWHERE in the line, and this repo's own commit
/// subjects match it: the formatted subject
///
/// ```text
/// 🐛  fix: pre-commit: stop hanging
/// ```
///
/// contains the fragment `pre-commit: s`, which satisfied `\w-\w+: \w`. So the
/// SUBJECT read as a footer, `group_footer` walked the whole message from the
/// bottom without ever hitting a non-footer line, and split at index 0 — the
/// emitted message became `["", subject, trailer…]`. git strips the leading
/// blank, which leaves the subject glued to the trailer block with no blank
/// line between them, and `%(trailers)` returns EMPTY for a commit that plainly
/// carries a `Co-Authored-By`. Silent, because the hook still exits 0.
///
/// Anchoring at column 0 keeps every real trailer (`Co-Authored-By: x`,
/// `Signed-off-by: y`) and rejects both `fix: pre-commit: stop hanging` and
/// prose like `see the pre-commit: docs above`, because in each of those the
/// leading key run stops at the first space and holds no hyphen.
fn is_hyphenated_key(line: &str) -> bool {
    let key: String = line
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    // A key starts and ends with a word character — `-foo: x` is a bulleted
    // list item, and `A-: x` was never a trailer either.
    if !key.starts_with(|c: char| c.is_alphanumeric() || c == '_')
        || !key.ends_with(|c: char| c.is_alphanumeric() || c == '_')
        || !key.contains('-')
    {
        return false;
    }
    let Some(rest) = line[key.len()..].strip_prefix(": ") else {
        return false;
    };
    rest.starts_with(|c: char| c.is_alphanumeric() || c == '_')
}

/// Separate the trailing run of footer lines, drop blanks inside it, and put
/// exactly one blank line before the group.
///
/// The run deliberately crosses BLANK lines, which is what merges a
/// `BREAKING CHANGE:` paragraph and a `Co-Authored-By:` paragraph into one
/// footer block. That is a feature, not an accident, so this is not a
/// "last paragraph only" rule.
///
/// The scan starts at `lines[1..]`: line 0 is the SUBJECT and can never be a
/// footer, whatever it happens to look like. Without that anchor a subject
/// misread as a footer (see `is_hyphenated_key`) let the run consume the entire
/// message, `split_at` became 0, and the subject was emitted INSIDE the footer
/// group with a blank line in front of it — destroying every trailer in the
/// commit. `lines[1..]` makes `split_at >= 1` by construction; `split('\n')`
/// never yields an empty vector, so the slice is always in range, and a
/// single-line message falls out of the general path producing exactly what it
/// produced before (`out = [subject, ""]`).
pub fn group_footer(text: &str) -> String {
    let trimmed = text.trim_end_matches('\n');
    let lines: Vec<&str> = trimmed.split('\n').collect();
    let mut footer_size = 0;
    for line in lines[1..].iter().rev() {
        if is_footer(line) {
            footer_size += 1;
        } else {
            break;
        }
    }
    let split_at = lines.len() - footer_size;
    let body = &lines[..split_at];
    let footer: Vec<&str> = lines[split_at..]
        .iter()
        .copied()
        .filter(|l| !l.is_empty())
        .collect();

    let mut out: Vec<&str> = body.to_vec();
    out.push("");
    out.extend(footer);
    format!("{}\n", out.join("\n"))
}

fn valid(msg: &str) {
    println!("  {} {msg}", valid_sign().trim());
}
fn error(msg: &str) {
    eprintln!("  {} {msg}", error_sign().trim());
}
fn orange(s: &str) -> String {
    highlight(s)
}

pub fn run(args: &[std::ffi::OsString]) -> Verdict {
    let Some(filename) = args.first().and_then(|a| a.to_str()) else {
        println!("Usage:\n\n./commit-msg <filename>");
        return Verdict::Block;
    };
    let Ok(raw) = std::fs::read_to_string(filename) else {
        return Verdict::Block;
    };
    let cleaned = strip_comments(&raw);
    let mut parts = cleaned.splitn(2, '\n');
    let subject_line = parts.next().unwrap_or("");
    let body = parts.next().unwrap_or("");

    if subject_line.is_empty() || subject_line.chars().count() > MAX_SUMMARY_LINE_SIZE {
        error(&format!(
            "Commit's first line should exist and be at most {} characters.",
            orange(&MAX_SUMMARY_LINE_SIZE.to_string())
        ));
        return Verdict::Block;
    }
    valid(&format!(
        "Summary size is at most {} characters",
        orange(&MAX_SUMMARY_LINE_SIZE.to_string())
    ));

    let types: Vec<String> = COMMIT_TYPES.iter().map(|t| orange(t.name)).collect();
    let Some(subject) = parse_subject(subject_line) else {
        error(&format!(
            "Commits MUST be prefixed with a type, which consists of a noun:
    {}
    The prefix must be followed by the OPTIONAL scope, OPTIONAL !,
    and REQUIRED terminal colon and space.
    A scope MAY be provided after a type. A scope MUST consist of a noun describing
    a section of the codebase surrounded by parenthesis, e.g., fix(parser)",
            types.join(", ")
        ));
        return Verdict::Block;
    };
    valid("A prefix is defined");

    if subject.description.is_empty() {
        error(&format!(
            "A description MUST immediately follow the {} and {} after the type/scope prefix.
    The description is a short summary of the code changes, e.g., fix: array parsing issue when multiple spaces were contained in string.",
            orange("colon"), orange("space")
        ));
        return Verdict::Block;
    }
    valid("A description is present in the summary");

    if subject.description.chars().count() > MAX_DESCRIPTION_SIZE {
        error(&format!(
            "The description after the {} should be at most {} characters.",
            orange("colon"),
            orange(&MAX_DESCRIPTION_SIZE.to_string())
        ));
        return Verdict::Block;
    }
    valid(&format!(
        "Description size is at most {} characters",
        orange(&MAX_DESCRIPTION_SIZE.to_string())
    ));

    // A fork that sends PRs upstream gets plain conventional commits: the emoji
    // would trip the upstream project's commit-lint and be unwelcome in the PR.
    let is_pr_repo = git::succeeds(&["remote", "get-url", "upstream"]);
    let emoji_prefix = if is_pr_repo {
        String::new()
    } else {
        format!("{}  ", vocabulary::emoji_for(&subject.prefix))
    };

    let formatted = format!(
        "{emoji_prefix}{}{}{}: {}\n\n{}\n",
        subject.prefix,
        subject.scope,
        subject.breaking,
        subject.description,
        wrap(&strip_comments(body), MAX_BODY_LINE_SIZE)
    );
    if std::fs::write(filename, group_footer(&formatted)).is_err() {
        return Verdict::Block;
    }
    Verdict::Proceed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_conventional_shapes() {
        let s = parse_subject("feat: add a thing").unwrap();
        assert_eq!(
            (s.prefix.as_str(), s.description.as_str()),
            ("feat", "add a thing")
        );

        let s = parse_subject("fix(parser): trim").unwrap();
        assert_eq!(s.scope, "(parser)");

        let s = parse_subject("fix(my-scope): trim").unwrap();
        assert_eq!(s.scope, "(my-scope)");

        let s = parse_subject("feat!: breaking").unwrap();
        assert_eq!(s.breaking, "!");
    }

    /// The bug the JS was patched for: only the BASE codepoint was consumed, so
    /// the trailing U+FE0F sat between emoji and type and the subject failed to
    /// match. Every one of these must parse.
    #[test]
    fn accepts_emoji_prefixes_including_multi_codepoint_ones() {
        for subject in [
            "✨ feat: x",
            "⬆️ chore: x",     // variation selector
            "♻️  refactor: x", // variation selector + two spaces
            "🔧  chore: x",
            "👨‍💻 feat: x", // ZWJ sequence
        ] {
            assert!(parse_subject(subject).is_some(), "failed: {subject}");
        }
    }

    #[test]
    fn rejects_what_is_not_a_conventional_subject() {
        assert!(parse_subject("just a message").is_none());
        assert!(parse_subject("feat add a thing").is_none()); // no colon
        assert!(parse_subject("feature: x").is_none()); // unknown type
        assert!(parse_subject("fix(bad scope): x").is_none()); // space in scope
    }

    #[test]
    fn description_may_be_empty_and_is_caught_by_the_caller() {
        assert_eq!(parse_subject("feat:").unwrap().description, "");
    }

    #[test]
    fn wraps_on_spaces_without_splitting_long_words() {
        let wrapped = wrap("aaa bbb ccc ddd", 7);
        assert_eq!(wrapped, "aaa bbb\nccc ddd");
        let long = "x".repeat(20);
        assert_eq!(wrap(&long, 7), long); // never split mid-word
    }

    #[test]
    fn recognises_footers() {
        assert!(is_footer("Co-Authored-By: someone"));
        assert!(is_footer("BREAKING CHANGE: it broke"));
        assert!(is_footer("Refs: #123"));
        assert!(is_footer(""));
        assert!(!is_footer("just prose"));
        assert!(!is_footer("a sentence with - a dash"));
    }

    /// The bare (no-colon) form, `Refs #123` / `Refs 123`, is intentionally
    /// also accepted — but "Refs" glued straight to a digit with no
    /// separator is prose, not a reference, and must not be swept into the
    /// footer group.
    #[test]
    fn a_bare_refs_needs_a_separator_not_just_a_leading_digit() {
        assert!(is_footer("Refs #123"));
        assert!(is_footer("Refs 123"));
        assert!(
            !is_footer("Refs42 was the original ticket."),
            "prose starting with Refs+digit must not read as a footer"
        );
    }

    /// A trailer key is only a trailer key when it STARTS its line.
    ///
    /// The formatted subject `fix: pre-commit: stop hanging` contains
    /// `pre-commit: s`, which the old anywhere-in-the-line rule accepted — and
    /// a subject read as a footer took the whole message down with it.
    #[test]
    fn a_key_must_start_the_line_to_be_a_footer() {
        // Real trailers, at column 0.
        assert!(is_footer("Co-Authored-By: someone"));
        assert!(is_footer("Signed-off-by: someone"));
        assert!(is_footer("Reviewed-by: a"));
        // The subject shape this repo writes constantly.
        assert!(!is_footer("fix: pre-commit: stop hanging"));
        assert!(!is_footer("🐛  fix: pre-commit: stop hanging"));
        // Prose that merely mentions a hyphenated word followed by a colon.
        assert!(!is_footer("see the pre-commit: docs above"));
        assert!(!is_footer("  Co-Authored-By: indented is not a trailer"));
        // A bullet is not a key, and neither is a key with nothing after the
        // hyphen.
        assert!(!is_footer("-foo: bar"));
        assert!(!is_footer("A-: bar"));
        // Neither branch below is anchored by this rule; both still work.
        assert!(is_footer("BREAKING CHANGE: it broke"));
        assert!(is_footer("Refs: #123"));
    }

    #[test]
    fn groups_the_trailing_footer_with_one_blank_line() {
        let out = group_footer("subject\n\nbody text\n\nCo-Authored-By: x\n\n");
        assert_eq!(out, "subject\n\nbody text\n\nCo-Authored-By: x\n");
    }

    /// The shapes a real message arrives in, for the property tests below.
    ///
    /// Every subject here is one that the old anywhere-in-the-line footer rule
    /// misread as a trailer, crossed with each body/footer arrangement the
    /// formatter emits.
    const SHAPES: &[&str] = &[
        // subject only
        "fix: pre-commit: stop hanging",
        "fix: pre-commit: stop hanging\n\n\n",
        // body only
        "fix: pre-commit: stop hanging\n\nthe worker thread blocked on a tty\n",
        // trailers only
        "fix: pre-commit: stop hanging\n\nCo-Authored-By: a <a@x>\n",
        // body and trailers
        "fix: pre-commit: stop hanging\n\nthe worker thread blocked\n\nCo-Authored-By: a <a@x>\n",
        // body, trailers, trailing blanks
        "fix: pre-commit: stop hanging\n\nthe worker thread blocked\n\nCo-Authored-By: a <a@x>\n\n\n",
        // two footer PARAGRAPHS, which the run deliberately merges
        "feat: x\n\nbody\n\nBREAKING CHANGE: it broke\n\nCo-Authored-By: a <a@x>\n",
        // an ordinary subject, to prove nothing regressed for the common case
        "feat: add a thing\n\nbody\n\nCo-Authored-By: a <a@x>\n",
    ];

    /// PROPERTY: grouping the footer never drops or invents a line, and never
    /// moves the subject off line 0.
    ///
    /// Both halves failed together for the subject `fix: pre-commit: stop
    /// hanging`: the whole message was swallowed into the footer group, blank
    /// body lines inside it were filtered away, and the emitted line 0 was the
    /// inserted blank rather than the subject.
    #[test]
    fn group_footer_never_loses_a_line() {
        for shape in SHAPES {
            let out = group_footer(shape);

            let mut before: Vec<&str> = shape
                .trim_end_matches('\n')
                .split('\n')
                .filter(|l| !l.is_empty())
                .collect();
            let mut after: Vec<&str> = out
                .trim_end_matches('\n')
                .split('\n')
                .filter(|l| !l.is_empty())
                .collect();
            before.sort_unstable();
            after.sort_unstable();
            assert_eq!(before, after, "lines changed for {shape:?} -> {out:?}");

            assert_eq!(
                out.split('\n').next(),
                shape.split('\n').next(),
                "the subject left line 0 for {shape:?} -> {out:?}"
            );
        }
    }

    /// PROPERTY: grouping is idempotent. A message that has already been
    /// formatted once — an amend, a rebase reword, a `--no-verify` retry — must
    /// come back byte for byte.
    #[test]
    fn group_footer_is_idempotent() {
        for shape in SHAPES {
            let once = group_footer(shape);
            let twice = group_footer(&once);
            assert_eq!(once, twice, "not idempotent for {shape:?}");
        }
    }

    /// The exact damage the anchor prevents: a blank line must stand between
    /// the subject and the footer group, and the subject must never appear
    /// inside it. Without the blank line git reads the trailer as a
    /// continuation of the subject and `%(trailers)` comes back empty.
    #[test]
    fn a_subject_and_its_trailers_stay_separated() {
        let out = group_footer("fix: pre-commit: stop hanging\n\nCo-Authored-By: a <a@x>\n");
        assert_eq!(
            out, "fix: pre-commit: stop hanging\n\nCo-Authored-By: a <a@x>\n",
            "got: {out:?}"
        );
        assert!(
            !out.starts_with('\n'),
            "the message must not begin with a blank line: {out:?}"
        );
    }

    #[test]
    fn strips_comment_lines() {
        assert_eq!(strip_comments("keep\n# drop\nkeep2"), "keep\nkeep2");
    }
}
