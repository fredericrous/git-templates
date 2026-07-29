//! pre-commit-merge-conflict — refuse staged files still carrying conflict
//! markers.

use super::common::{fail, hl, ok};
use crate::git;

/// The markers, BUILT rather than written.
///
/// A file that greps for conflict markers cannot contain them literally, or it
/// flags itself — which is exactly what happened the first time this was
/// ported, caught by the hook running over this very commit. The shell version
/// solved it by excluding its own path and its test's; constructing the strings
/// removes the need for any exclusion list, so a future rename cannot silently
/// reintroduce the problem.
fn markers() -> [String; 3] {
    ["<", "=", ">"].map(|c| c.repeat(7))
}

pub fn run(_hook_name: &str, _args: &[std::ffi::OsString]) -> i32 {
    let m = markers();
    // --all-match: only files containing ALL THREE, so a lone rule of `=`
    // characters in a document is not mistaken for a conflict.
    let args: Vec<&str> = vec![
        "grep",
        "--cached",
        "-e",
        &m[0],
        "--or",
        "-e",
        &m[1],
        "--or",
        "-e",
        &m[2],
        "--all-match",
        "--files-with-matches",
    ];
    let found = git::stdout(&args).unwrap_or_default();

    let files: Vec<&str> = found
        .lines()
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .collect();

    if !files.is_empty() {
        fail(&format!(
            "Merge conflict detected in {}",
            hl(&files.join(", "))
        ));
        return 1;
    }
    ok("No merge confict detected");
    0
}

#[cfg(test)]
mod tests {
    use super::markers;

    #[test]
    fn the_markers_are_the_real_seven_character_ones() {
        let m = markers();
        assert_eq!(m[0].len(), 7);
        assert_eq!(m[0], "<".repeat(7));
        assert_eq!(m[1], "=".repeat(7));
        assert_eq!(m[2], ">".repeat(7));
    }

    /// The point of building them: this source file must not contain a literal
    /// marker, or the hook flags its own implementation.
    #[test]
    fn this_file_contains_no_literal_marker() {
        let src = include_str!("merge_conflict.rs");
        for m in markers() {
            assert!(!src.contains(&m), "literal marker present in this file");
        }
    }
}
