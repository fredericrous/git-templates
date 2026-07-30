//! The refs git feeds `pre-push` on stdin, read ONCE and shared.
//!
//! git writes one line per ref being pushed:
//!
//! ```text
//! <local ref> <local oid> <remote ref> <remote oid>
//! ```
//!
//! This exists because stdin can only be consumed once. While checks were
//! separate processes with INHERITED stdin, whichever ran first drained it and
//! the rest saw EOF — silently. Two repos in the fleet had a custom
//! `pre-push-branch-protect.sh` whose `while read` loop sorted BEFORE
//! `pre-push-run-tests-js`, so the test gate received no refs and ran nothing.
//! Nobody noticed, because "no refs" and "nothing to test" look identical.
//!
//! Reading it in one place and lending the result to every check removes that
//! whole class of bug, and makes it impossible to reintroduce by adding a
//! second stdin reader.

use std::io::BufRead;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushRef {
    pub local_ref: String,
    pub local_oid: String,
    pub remote_ref: String,
    pub remote_oid: String,
}

/// Lazily-read pushed refs. `OnceLock` rather than `OnceCell` because
/// pre-commit runs its checks on threads and `Ctx` must therefore be `Sync`.
#[derive(Default)]
pub struct PushRefs(OnceLock<Vec<PushRef>>);

impl PushRefs {
    /// Read on first use. A check that never asks never blocks on stdin —
    /// which matters, because pre-commit's stdin is not a ref list.
    pub fn get(&self) -> &[PushRef] {
        self.0.get_or_init(|| parse(std::io::stdin().lock()))
    }
}

pub fn parse<R: BufRead>(r: R) -> Vec<PushRef> {
    r.lines()
        .map_while(Result::ok)
        .filter_map(|line| {
            let mut f = line.split_whitespace();
            let (a, b, c, d) = (f.next()?, f.next()?, f.next()?, f.next()?);
            Some(PushRef {
                local_ref: a.to_owned(),
                local_oid: b.to_owned(),
                remote_ref: c.to_owned(),
                remote_oid: d.to_owned(),
            })
        })
        .collect()
}

/// Every path touched by the refs being pushed.
///
/// Shared because two callers need exactly this list and would otherwise write
/// the zero-oid and range handling twice: `cargo-test` decides whether a suite
/// is worth running, and a declared pre-push check decides whether its `scope`
/// applies. A copy that got the delete case wrong would run a test suite on a
/// branch deletion.
pub fn changed_files(refs: &[PushRef]) -> Vec<String> {
    let zero = crate::git::stdout(&["hash-object", "--stdin"])
        .map(|h| "0".repeat(h.len()))
        .unwrap_or_else(|| "0".repeat(40));
    let mut changed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for r in refs {
        if r.local_oid == zero {
            continue; // deleting a ref pushes no code
        }
        let range = if r.remote_oid == zero {
            r.local_oid.clone()
        } else {
            format!("{}..{}", r.remote_oid, r.local_oid)
        };
        if let Some(out) =
            crate::git::stdout(&["diff-tree", "--no-commit-id", "--name-only", "-r", &range])
        {
            changed.extend(out.lines().map(str::trim).map(str::to_owned));
        }
    }
    changed.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_four_fields() {
        let got = parse(&b"refs/heads/x aaa refs/heads/y bbb\n"[..]);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].local_ref, "refs/heads/x");
        assert_eq!(got[0].remote_ref, "refs/heads/y");
        assert_eq!(got[0].remote_oid, "bbb");
    }

    #[test]
    fn several_refs_and_junk_lines() {
        let got = parse(&b"a 1 b 2\ngarbage\n\nc 3 d 4\n"[..]);
        assert_eq!(got.len(), 2, "short lines are skipped, not fatal");
        assert_eq!(got[1].local_ref, "c");
    }

    #[test]
    fn empty_stdin_is_no_refs_not_an_error() {
        assert!(parse(&b""[..]).is_empty());
    }
}
