//! pre-commit-package-lock — keep package.json and its lockfile in step.
//!
//! Scoped per directory: a package.json that is not a real npm project (no
//! lockfile beside it — e.g. the `.git/hooks/package.json` type-marker) never
//! demands one, and in a monorepo one project's lockfile does not satisfy
//! another's.

use super::common::{fail, hl, ok, repo_root, staged_files, warn};
use std::io::{BufRead, BufReader};
use std::path::Path;

fn dir_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => ".",
    }
}

fn sibling(dir: &str, name: &str) -> String {
    if dir == "." {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

pub struct Verdict {
    /// package.json staged, a real lockfile exists on disk but is not staged.
    pub forgot_lock: Vec<String>,
    /// lockfile staged without its package.json.
    pub orphan_lock: Vec<String>,
}

pub fn classify(staged: &[String], lock_exists: impl Fn(&str) -> bool) -> Verdict {
    let is_staged = |p: &str| staged.iter().any(|s| s == p);
    let mut v = Verdict {
        forgot_lock: Vec::new(),
        orphan_lock: Vec::new(),
    };
    for f in staged {
        let dir = dir_of(f);
        let base = f.rsplit('/').next().unwrap_or(f);
        match base {
            "package.json" => {
                let lock = sibling(dir, "package-lock.json");
                if is_staged(&lock) {
                    continue; // both staged → in sync
                }
                if lock_exists(&lock) {
                    v.forgot_lock.push(f.clone());
                }
            }
            "package-lock.json" => {
                let pkg = sibling(dir, "package.json");
                if !is_staged(&pkg) {
                    v.orphan_lock.push(f.clone());
                }
            }
            _ => {}
        }
    }
    v
}

/// Ask on the terminal, when there is one. Never crash without it: the old
/// `read < /dev/tty` aborted non-interactive commits with "device not
/// configured". Headless callers bypass with `git -c hook.skip=package-lock`.
#[cfg(unix)]
fn confirm() -> bool {
    let Ok(tty) = std::fs::File::open("/dev/tty") else {
        return false;
    };
    print!("  Commit anyway? (y/N) ");
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if BufReader::new(tty).read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim_start().chars().next(), Some('y') | Some('Y'))
}

/// Windows has no /dev/tty; treat it as non-interactive rather than inventing
/// a console API for a confirmation prompt.
#[cfg(not(unix))]
fn confirm() -> bool {
    false
}

pub fn run(_args: &[std::ffi::OsString]) -> i32 {
    let staged = staged_files(&[]);
    let root = repo_root();
    let v = classify(&staged, |lock| Path::new(&root).join(lock).is_file());

    if v.forgot_lock.is_empty() && v.orphan_lock.is_empty() {
        ok("package.json & package-lock.json look in sync");
        return 0;
    }
    for f in &v.orphan_lock {
        fail(&format!("{} staged without its package.json", hl(f)));
    }
    for f in &v.forgot_lock {
        warn(&format!(
            "{} changed but its package-lock.json is not staged",
            hl(f)
        ));
    }

    // An orphan lock is a hard error; a merely-forgotten one can be confirmed past.
    if v.orphan_lock.is_empty() && confirm() {
        return 0;
    }
    fail(&format!(
        "Run {} and stage the lockfile, or bypass with {}",
        hl("npm install"),
        hl("git -c hook.skip=package-lock commit")
    ));
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str], locks: &[&str]) -> Verdict {
        let staged: Vec<String> = s.iter().map(|x| x.to_string()).collect();
        let locks: Vec<String> = locks.iter().map(|x| x.to_string()).collect();
        classify(&staged, move |l| locks.iter().any(|x| x == l))
    }

    #[test]
    fn both_staged_is_in_sync() {
        let r = v(
            &["package.json", "package-lock.json"],
            &["package-lock.json"],
        );
        assert!(r.forgot_lock.is_empty() && r.orphan_lock.is_empty());
    }

    #[test]
    fn package_json_alone_with_a_real_lock_on_disk_is_a_forgotten_lock() {
        let r = v(&["package.json"], &["package-lock.json"]);
        assert_eq!(r.forgot_lock, vec!["package.json".to_string()]);
    }

    /// The `.git/hooks/package.json` type-marker case: no lockfile beside it,
    /// so it is not an npm project and must not demand one.
    #[test]
    fn package_json_without_a_lock_on_disk_demands_nothing() {
        let r = v(&["package.json"], &[]);
        assert!(r.forgot_lock.is_empty() && r.orphan_lock.is_empty());
    }

    #[test]
    fn a_lock_without_its_package_json_is_an_orphan() {
        let r = v(&["package-lock.json"], &["package-lock.json"]);
        assert_eq!(r.orphan_lock, vec!["package-lock.json".to_string()]);
    }

    /// In a monorepo, one project's lockfile does not satisfy another's.
    #[test]
    fn scoping_is_per_directory() {
        let r = v(
            &["apps/a/package.json", "apps/b/package-lock.json"],
            &["apps/a/package-lock.json", "apps/b/package-lock.json"],
        );
        assert_eq!(r.forgot_lock, vec!["apps/a/package.json".to_string()]);
        assert_eq!(r.orphan_lock, vec!["apps/b/package-lock.json".to_string()]);
    }
}
