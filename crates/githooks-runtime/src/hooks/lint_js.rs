//! pre-commit-lint-js — ESLint over staged JS/TS, scoped to repos that lint.

use super::common::{
    fail, first_existing, hl, ok, repo_root, resolve_tool, run as run_tool, staged_files, warn,
};

pub fn run(args: &[std::ffi::OsString]) -> i32 {
    let files = staged_files(&[".js", ".jsx", ".ts", ".tsx", ".vue"]);
    if files.is_empty() {
        return 0;
    }
    let root = repo_root();

    // ESLint 9+ ERRORS OUT when it finds no config ("couldn't find an
    // eslint.config file") instead of no-op'ing, which would fail commits in
    // repos that do not lint JS at all — infra and manifest repos, and this
    // templates repo. So the config is the opt-in signal.
    let has_config = first_existing(
        &root,
        &[
            "eslint.config.js",
            "eslint.config.mjs",
            "eslint.config.cjs",
            "eslint.config.ts",
            ".eslintrc",
            ".eslintrc.js",
            ".eslintrc.cjs",
            ".eslintrc.json",
            ".eslintrc.yml",
            ".eslintrc.yaml",
        ],
    )
    .is_some()
        || std::fs::read_to_string(format!("{root}/package.json"))
            .map(|p| p.contains("\"eslintConfig\""))
            .unwrap_or(false);
    if !has_config {
        ok("ESLint skipped (no eslint config)");
        return 0;
    }

    let Some(argv) = resolve_tool(&root, "eslint") else {
        // A non-zero exit from a MISSING eslint is not the same as lint errors,
        // so warn and skip rather than block with a false "issues found".
        warn(&format!(
            "eslint config found but no eslint binary. Run {}",
            hl("npm install")
        ));
        return 0;
    };

    let mut extra = files;
    extra.extend(args.iter().filter_map(|a| a.to_str()).map(str::to_owned));
    if !run_tool(&root, &argv, &extra) {
        fail("ESLint issues found. Please fix");
        return 1;
    }
    ok("ESLint passed");
    0
}
