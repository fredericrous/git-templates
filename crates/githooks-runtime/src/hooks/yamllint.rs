//! pre-commit-yamllint — strict YAML lint, but only where a repo has opted in.

use super::common::run as run_tool;
use super::common::{fail, first_existing, hl, ok, repo_root, staged_files, warn, which};

pub fn run(_args: &[std::ffi::OsString]) -> i32 {
    let files = staged_files(&[".yaml", ".yml"]);
    if files.is_empty() {
        return 0;
    }
    if which("yamllint").is_none() {
        warn(&format!(
            "YAML files detected. To strict-lint them, install {}",
            hl("yamllint")
        ));
        return 0;
    }
    let root = repo_root();
    // yamllint's stock rules are too noisy to enforce generically, so a
    // repo-local config is the opt-in signal. Skip silently without one.
    let Some(config) = first_existing(&root, &[".yamllint.yaml", ".yamllint.yml", ".yamllint"])
    else {
        return 0;
    };
    let argv = vec!["yamllint".to_string(), "-c".to_string(), config];
    if !run_tool(&root, &argv, &files) {
        fail("yamllint found issues. Please fix");
        return 1;
    }
    ok("yamllint passed");
    0
}
