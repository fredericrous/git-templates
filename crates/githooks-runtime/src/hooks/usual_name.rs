//! pre-commit-usual-name — warn the first time an author commits under a
//! given name/email, so a misconfigured `user.name` is noticed at the first
//! commit rather than after twenty.
//!
//! Warning only: it never blocks a commit.

use crate::git;
use crate::ui::{highlight, warning_sign};

pub fn run(_args: &[std::ffi::OsString]) -> i32 {
    // An empty repo has no history to compare against.
    if !git::succeeds(&["log", "-1"]) {
        return 0;
    }

    let name = git::stdout(&["config", "user.name"]).unwrap_or_default();
    let email = git::stdout(&["config", "user.email"]).unwrap_or_default();
    let full = format!("{name} <{email}>");

    // FIXED-STRING containment, never a pattern: a real name can carry regex
    // metacharacters (O'Brien, "Foo (Bar)") and as a pattern those would
    // misfire — `(dev)` is a group, and would match an author who never
    // committed. This is why the shell version used rg -F / grep -F.
    let seen = git::stdout(&["shortlog", "-s", "-n", "-e", "--all"])
        .is_some_and(|log| log.contains(&full));

    if !seen {
        println!(
            "{} It is the first time you commit as {}",
            warning_sign(),
            highlight(&full)
        );
    }
    0
}

#[cfg(test)]
mod tests {
    /// The property that matters, isolated from git: containment is literal.
    fn seen(log: &str, full: &str) -> bool {
        log.contains(full)
    }

    #[test]
    fn matches_an_existing_author_literally() {
        let log = "    12\ttest all mighty <test@domain.test>\n     3\tOther <o@x.test>";
        assert!(seen(log, "test all mighty <test@domain.test>"));
        assert!(!seen(log, "test mighty <test@domain.test>"));
    }

    /// As a REGEX, "test (dev) all mighty" would match "test dev all mighty"
    /// via the group — the false negative that lets a misconfigured identity
    /// pass unnoticed. Containment cannot do that.
    #[test]
    fn regex_metacharacters_stay_literal() {
        let log = "     1\ttest dev all mighty <t@x.test>";
        assert!(!seen(log, "test (dev) all mighty <t@x.test>"));

        let log2 = "     1\ttest (dev) all mighty <t@x.test>";
        assert!(seen(log2, "test (dev) all mighty <t@x.test>"));
    }
}
