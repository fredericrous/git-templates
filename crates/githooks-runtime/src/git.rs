//! Thin wrappers over the `git` calls the hooks make.

use std::process::{Command, Stdio};

/// stdout of a git command, trimmed. `None` when git itself failed — which the
/// hooks treat as "cannot tell, do not block", never as "empty".
pub fn stdout(args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The same, run inside `dir`.
///
/// The dashboard asks about repositories it is not standing in, and must get
/// the answer git would give THERE — config is per-repository, so asking from
/// the wrong directory returns the wrong severity.
pub fn stdout_in(dir: &std::path::Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// stdout of a git command that itself reads a list from stdin — `diff-tree
/// --stdin`, fed a list of commits, is the only caller today. Lossy but
/// untrimmed: every line is a path, and the caller trims those itself.
pub fn stdout_piped(args: &[&str], stdin: &str) -> Option<String> {
    use std::io::Write;
    let mut child = Command::new("git")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(stdin.as_bytes()).ok()?;
    let out = child.wait_with_output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// As `stdout_piped`, but returning the RAW bytes.
///
/// Needed by the one caller that must both feed git a list on stdin and read a
/// `-z` path list back — `diff-tree --stdin -z`. `stdout_paths` cannot serve it
/// (no stdin) and `stdout_piped` cannot either (lossy `String`, and the NUL
/// separators are the whole point).
pub fn stdout_piped_raw(args: &[&str], stdin: &str) -> Option<Vec<u8>> {
    use std::io::Write;
    let mut child = Command::new("git")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(stdin.as_bytes()).ok()?;
    let out = child.wait_with_output().ok()?;
    out.status.success().then_some(out.stdout)
}

/// As `stdout_piped`, but inside `dir` and taking raw bytes.
///
/// `-C dir` matters for `hash-object`: a repository configured for SHA-256
/// computes a different id than the default, so the identity has to be asked
/// of THAT repository. Bytes rather than `&str` because the input is a file we
/// have already read and must not re-encode.
pub fn stdout_piped_in(dir: &std::path::Path, args: &[&str], stdin: &[u8]) -> Option<String> {
    use std::io::Write;
    let mut child = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(stdin).ok()?;
    let out = child.wait_with_output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Raw stdout, untrimmed and not lossy — for a patch, where a trailing newline
/// and any byte in a binary hunk are load-bearing.
pub fn stdout_raw(args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("git")
        .args(args)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

/// True when the command exits 0. Output discarded.
pub fn succeeds(args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// A path list from `diff --name-only`, `diff-tree --name-only` or
/// `ls-files` — commands whose output is meant to be split into individual
/// paths, never just read as one blob.
///
/// By default git QUOTES any "unusual" byte in a path, non-ASCII included:
/// `é.json` prints as `"\303\251.json"`. Reading that line as a literal path
/// looks up a file that does not exist — the caller then treats real,
/// unstaged content as absent, which is how `StagedOnly` used to lose it.
/// `-z` disables quoting entirely and NUL-terminates each entry instead, so
/// there is no escaping left to get wrong. Inserted right after the
/// subcommand (`args[0]`), which is always a valid position for it on every
/// command this is used for.
pub fn stdout_paths(args: &[&str]) -> Option<Vec<String>> {
    let (first, rest) = args.split_first()?;
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push(*first);
    argv.push("-z");
    argv.extend_from_slice(rest);
    stdout_raw(&argv).map(|raw| split_nul_paths(&raw))
}

/// The parsing half of [`stdout_paths`], split out so it can be tested on
/// literal bytes rather than a real git process — including the byte
/// sequence a QUOTED path would have produced under the old line-splitting
/// approach, to prove `-z` output is never reinterpreted that way.
pub(crate) fn split_nul_paths(raw: &[u8]) -> Vec<String> {
    raw.split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_nul_and_drops_the_trailing_empty_segment() {
        assert_eq!(
            split_nul_paths(b"src/main.rs\0Cargo.toml\0"),
            vec!["src/main.rs", "Cargo.toml"]
        );
    }

    #[test]
    fn empty_input_is_no_paths() {
        assert_eq!(split_nul_paths(b""), Vec::<String>::new());
    }

    /// The exact bug this exists to prevent: under `--name-only` without
    /// `-z`, git would have printed `é.json` as the quoted, LINE-oriented
    /// text `"\303\251.json"` — literal backslashes, digits and quotes, nine
    /// bytes standing in for the original two-byte UTF-8 sequence. `-z`
    /// output carries the real UTF-8 bytes of the path with no such
    /// reinterpretation, so splitting on NUL must hand them back unchanged.
    #[test]
    fn a_non_ascii_path_is_not_reinterpreted_as_its_quoted_form() {
        let mut raw = "é.json".as_bytes().to_vec();
        raw.push(0);
        let got = split_nul_paths(&raw);
        assert_eq!(got, vec!["é.json".to_string()]);
        assert_ne!(got[0], "\"\\303\\251.json\"", "must not be the quoted form");
    }
}
