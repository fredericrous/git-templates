//! commit-msg — validate the summary line and reformat the message.
//!
//! Validates: subject present and ≤72; a conventional type prefix; a
//! description; description ≤50. Formats: prepend the type's gitmoji, hard-wrap
//! the body at 72, and group the trailing footers with one blank line before
//! them.
//!
//! Ported from ~190 lines of JS. The one structural simplification is how the
//! optional leading emoji is recognised — see `split_leading_emoji`.

use crate::git;
use crate::ui::color;

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
    if let Some(rest) = line
        .strip_prefix("Refs:")
        .or_else(|| line.strip_prefix("Refs"))
    {
        let r = rest.trim_start_matches(' ');
        let r = r.strip_prefix('#').unwrap_or(r);
        if r.starts_with(|c: char| c.is_ascii_digit()) {
            return true;
        }
    }
    // /\w-\w{1,}:\s\w/ — a hyphenated key followed by ": " and a word char.
    let b: Vec<char> = line.chars().collect();
    for i in 0..b.len() {
        if b[i] == '-' && i > 0 && (b[i - 1].is_alphanumeric() || b[i - 1] == '_') {
            let mut j = i + 1;
            while j < b.len() && (b[j].is_alphanumeric() || b[j] == '_') {
                j += 1;
            }
            if j > i + 1 && j + 2 < b.len() && b[j] == ':' && b[j + 1] == ' ' {
                let c = b[j + 2];
                if c.is_alphanumeric() || c == '_' {
                    return true;
                }
            }
        }
    }
    false
}

/// Separate the trailing run of footer lines, drop blanks inside it, and put
/// exactly one blank line before the group.
pub fn group_footer(text: &str) -> String {
    let trimmed = text.trim_end_matches('\n');
    let lines: Vec<&str> = trimmed.split('\n').collect();
    let mut footer_size = 0;
    for line in lines.iter().rev() {
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
    println!("  {} {msg}", color("\u{2713}", "112"));
}
fn error(msg: &str) {
    eprintln!("  {} {msg}", color("\u{2717}", "160"));
}
fn orange(s: &str) -> String {
    color(s, "208")
}

pub fn run(args: &[std::ffi::OsString]) -> i32 {
    let Some(filename) = args.first().and_then(|a| a.to_str()) else {
        println!("Usage:\n\n./commit-msg <filename>");
        return 1;
    };
    let Ok(raw) = std::fs::read_to_string(filename) else {
        return 1;
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
        return 1;
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
        return 1;
    };
    valid("A prefix is defined");

    if subject.description.is_empty() {
        error(&format!(
            "A description MUST immediately follow the {} and {} after the type/scope prefix.
    The description is a short summary of the code changes, e.g., fix: array parsing issue when multiple spaces were contained in string.",
            orange("colon"), orange("space")
        ));
        return 1;
    }
    valid("A description is present in the summary");

    if subject.description.chars().count() > MAX_DESCRIPTION_SIZE {
        error(&format!(
            "The description after the {} should be at most {} characters.",
            orange("colon"),
            orange(&MAX_DESCRIPTION_SIZE.to_string())
        ));
        return 1;
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
        return 1;
    }
    0
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

    #[test]
    fn groups_the_trailing_footer_with_one_blank_line() {
        let out = group_footer("subject\n\nbody text\n\nCo-Authored-By: x\n\n");
        assert_eq!(out, "subject\n\nbody text\n\nCo-Authored-By: x\n");
    }

    #[test]
    fn strips_comment_lines() {
        assert_eq!(strip_comments("keep\n# drop\nkeep2"), "keep\nkeep2");
    }
}
