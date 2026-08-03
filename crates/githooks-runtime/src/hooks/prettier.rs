//! pre-commit-prettier — format check over staged files, scoped to repos that
//! actually opt into prettier.

use super::common::{
    fail, first_existing, fixing_enabled, hl, ok, repo_root, resolve_tool, restage,
    run as run_tool, staged_files, warn,
};
use crate::check::Outcome;
use std::path::Path;
use std::process::{Command, Stdio};

const EXTS: [&str; 17] = [
    ".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs", ".json", ".jsonc", ".css", ".scss", ".less",
    ".html", ".vue", ".md", ".mdx", ".yaml", ".yml",
];

/// A repo opts in via a prettier config file or a `"prettier"` key in
/// package.json. Without that signal the hook does nothing — in a repo that
/// does not use prettier, `npx prettier` would download it and flag every file
/// against defaults.
fn has_config(root: &str) -> bool {
    if first_existing(
        root,
        &[
            ".prettierrc",
            "prettier.config.js",
            "prettier.config.mjs",
            "prettier.config.cjs",
            ".prettierrc.json",
            ".prettierrc.yml",
            ".prettierrc.yaml",
            ".prettierrc.js",
            ".prettierrc.cjs",
            ".prettierrc.mjs",
            ".prettierrc.toml",
        ],
    )
    .is_some()
    {
        return true;
    }
    std::fs::read_to_string(Path::new(root).join("package.json"))
        .map(|p| p.contains("\"prettier\""))
        .unwrap_or(false)
}

pub fn run(_args: &[std::ffi::OsString]) -> Outcome {
    let files = staged_files(&EXTS);
    if files.is_empty() {
        return Outcome::Passed;
    }
    let root = repo_root();

    let tool = resolve_tool(&root, "prettier");
    // A pinned local prettier is itself an opt-in signal; without config AND
    // without a local binary the repo simply does not use prettier.
    let local_pinned = tool
        .as_ref()
        .map(|t| t[0].contains("node_modules/.bin"))
        .unwrap_or(false);
    if !has_config(&root) && !local_pinned {
        return Outcome::Passed;
    }
    let Some(argv) = tool else {
        warn(&format!(
            "prettier config found but no prettier binary. Run {}",
            hl("npm install")
        ));
        return Outcome::Unavailable;
    };

    // Ignore a GLOBAL ~/.editorconfig when this repo defines none of its own:
    // prettier otherwise walks up past the repo and enforces machine-wide
    // defaults (4-space, say) on a repo formatted differently — a false
    // positive that also disagrees with CI, which has no ~/.editorconfig.
    // A repo-level .editorconfig is still honoured, matching CI.
    let mut flags: Vec<String> = Vec::new();
    if !Path::new(&root).join(".editorconfig").is_file() {
        flags.push("--no-editorconfig".into());
    }

    // `--` before the file list at each of these: a staged file named e.g.
    // `-x.js` would otherwise be read as a flag by prettier's own parser —
    // and prettier does not even error on that, it exits 0 having checked
    // nothing, so the file would silently never be linted at all.
    let mut check = flags.clone();
    check.push("--check".into());
    check.push("--".into());
    check.extend(files.iter().cloned());
    if !run_quiet(&root, &argv, &check) && fixing_enabled() {
        // Asked to repair, so repair rather than reporting an instruction the
        // author would carry out identically by hand.
        let mut write = flags.clone();
        write.push("--write".into());
        write.push("--".into());
        write.extend(files.iter().cloned());
        if run_quiet(&root, &argv, &write) && restage(&files) {
            ok("Prettier reformatted and re-staged");
            return Outcome::Fixed;
        }
    }
    if !run_quiet(&root, &argv, &check) {
        fail(&format!(
            "Prettier found unformatted files. Run {} on:",
            hl("prettier --write")
        ));
        let mut list = flags;
        list.push("--list-different".into());
        list.push("--".into());
        list.extend(files);
        let _ = run_tool(&root, &argv, &list);
        return Outcome::Failed;
    }
    ok("Prettier passed");
    Outcome::Passed
}

/// Like `common::run` but silent — the check pass only decides the verdict; the
/// offending files are printed by the `--list-different` pass after it.
fn run_quiet(root: &str, argv: &[String], extra: &[String]) -> bool {
    let Some((program, rest)) = argv.split_first() else {
        return true;
    };
    Command::new(program)
        .args(rest)
        .args(extra)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
