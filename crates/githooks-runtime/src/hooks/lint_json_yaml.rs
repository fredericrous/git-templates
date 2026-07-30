//! pre-commit-lint-json-yaml — parse staged JSON/YAML so a syntax error never
//! reaches the repo. Both linters soft-fail when absent: warn, don't block.

use super::common::{fail, hl, ok, repo_root, staged_files, warn, which};
use crate::check::Outcome;
use std::path::Path;
use std::process::{Command, Stdio};

/// Helm chart templates carry Go templating (`{{ }}`) and are not valid YAML
/// until Helm renders them. A staged YAML under a chart's `templates/` — i.e.
/// with a sibling `Chart.yaml` at the chart root — is skipped, or valid chart
/// commits would need --no-verify just to get past this hook.
pub fn is_helm_template(root: &str, file: &str) -> bool {
    let Some(i) = file.find("/templates/") else {
        return false;
    };
    Path::new(root)
        .join(&file[..i])
        .join("Chart.yaml")
        .is_file()
}

fn parses(root: &str, tool: &str, args: &[&str]) -> bool {
    Command::new(super::common::program(tool))
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn run(_args: &[std::ffi::OsString]) -> Outcome {
    let json: Vec<String> = staged_files(&[".json"]);
    let yaml: Vec<String> = staged_files(&[".yaml"]);
    if json.is_empty() && yaml.is_empty() {
        return Outcome::Passed;
    }
    let root = repo_root();
    // Two independent halves. A missing parser silences one of them, and a
    // half that never ran must not be reported as a half that passed — so
    // both a failure anywhere and a gap anywhere outrank the clean case.
    let mut failed = false;
    let mut unavailable = false;

    if !json.is_empty() {
        if which("node").is_some() {
            for f in &json {
                let script = r#"JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"))"#;
                if !parses(&root, "node", &["-e", script, f]) {
                    fail(&format!("Invalid JSON: {}", hl(f)));
                    failed = true;
                }
            }
        } else {
            warn(&format!(
                "JSON files detected. To lint them, install {}",
                hl("node")
            ));
            unavailable = true;
        }
    }

    if !yaml.is_empty() {
        if which("yq").is_some() {
            for f in &yaml {
                if is_helm_template(&root, f) {
                    continue;
                }
                if !parses(&root, "yq", &["e", "true", f]) {
                    fail(&format!("Invalid YAML: {}", hl(f)));
                    failed = true;
                }
            }
        } else {
            warn(&format!(
                "YAML files detected. To lint them, install {}",
                hl("yq")
            ));
            unavailable = true;
        }
    }

    match (failed, unavailable) {
        (true, _) => Outcome::Failed,
        (false, true) => Outcome::Unavailable,
        (false, false) => {
            ok("Json/Yaml Lint passed");
            Outcome::Passed
        }
    }
}
