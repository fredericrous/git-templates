//! pre-commit-lint-json-yaml — parse staged JSON/YAML so a syntax error never
//! reaches the repo. Both linters soft-fail when absent: warn, don't block.

use super::common::{fail, hl, ok, repo_root, staged_files, warn, which};
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

fn parses(root: &str, program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn run(_args: &[std::ffi::OsString]) -> i32 {
    let json: Vec<String> = staged_files(&[".json"]);
    let yaml: Vec<String> = staged_files(&[".yaml"]);
    if json.is_empty() && yaml.is_empty() {
        return 0;
    }
    let root = repo_root();
    let mut rc = 0;

    if !json.is_empty() {
        if which("node").is_some() {
            for f in &json {
                let script = r#"JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"))"#;
                if !parses(&root, "node", &["-e", script, f]) {
                    fail(&format!("Invalid JSON: {}", hl(f)));
                    rc = 1;
                }
            }
        } else {
            warn(&format!(
                "JSON files detected. To lint them, install {}",
                hl("node")
            ));
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
                    rc = 1;
                }
            }
        } else {
            warn(&format!(
                "YAML files detected. To lint them, install {}",
                hl("yq")
            ));
        }
    }

    if rc == 0 {
        ok("Json/Yaml Lint passed");
    }
    rc
}
