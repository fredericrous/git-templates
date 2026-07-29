//! The tool-driven pre-commit hooks: lint-json-yaml, yamllint, lint-js,
//! prettier, ruff, pyright.
//!
//! Every one is SCOPED — it fires only when the repo opts in via a config —
//! and SOFT — a missing tool warns and skips rather than blocking a commit.
//! Those two properties are what keep a Python repo from pulling eslint, so
//! they are tested first and without needing the tool present.

mod common;
use common::{missing, Repo};

/// A Helm template body. `a: {{ .Values.x }}` is NOT usable here — it happens
/// to be valid flow-mapping YAML, so yq accepts it and the "outside a chart
/// still fails" case would pass for the wrong reason. A conditional BLOCK is
/// what yq genuinely rejects, which is why the zsh suite used one.
const HELM_TMPL: &str =
    "{{- if .Values.enabled }}\nkind: Deployment\nmetadata:\n  name: x\n{{- end }}\n";

// ---- lint-json-yaml -----------------------------------------------------

#[test]
fn invalid_json_is_rejected_and_valid_json_passes() {
    if missing("node") {
        return;
    }
    let r = Repo::new();
    r.stage("bad.json", "{\"a\": 1,,}\n");
    assert!(!r.hook("pre-commit-lint-json-yaml", &[]).passed());

    let r = Repo::new();
    r.stage("ok.json", "{\"a\": 1}\n");
    assert!(r.hook("pre-commit-lint-json-yaml", &[]).passed());
}

#[test]
fn invalid_yaml_is_rejected_and_valid_yaml_passes() {
    if missing("yq") {
        return;
    }
    let r = Repo::new();
    r.stage("bad.yaml", "a:\n\tb: 1\n"); // tab indentation
    assert!(!r.hook("pre-commit-lint-json-yaml", &[]).passed());

    let r = Repo::new();
    r.stage("ok.yaml", "a:\n  b: 1\n");
    assert!(r.hook("pre-commit-lint-json-yaml", &[]).passed());
}

/// Helm chart templates carry Go templating and are not valid YAML until Helm
/// renders them. Without this carve-out every valid chart commit would need
/// --no-verify.
#[test]
fn a_helm_chart_template_is_skipped() {
    if missing("yq") {
        return;
    }
    let r = Repo::new();
    r.write("chart/Chart.yaml", "name: c\n");
    r.stage("chart/templates/deploy.yaml", HELM_TMPL);
    assert!(r.hook("pre-commit-lint-json-yaml", &[]).passed());
}

/// …but the same Go-template YAML OUTSIDE a chart is still invalid.
#[test]
fn go_template_yaml_outside_a_chart_still_fails() {
    if missing("yq") {
        return;
    }
    let r = Repo::new();
    r.stage("k/deploy.yaml", HELM_TMPL);
    assert!(!r.hook("pre-commit-lint-json-yaml", &[]).passed());
}

// ---- yamllint -----------------------------------------------------------

/// Stock yamllint rules are too noisy to enforce generically, so a repo-local
/// config is the opt-in signal. Without one the hook must do nothing.
#[test]
fn yamllint_does_nothing_without_a_repo_config() {
    let r = Repo::new();
    r.stage("a.yaml", "a:   1\n");
    let run = r.hook("pre-commit-yamllint", &[]);
    assert!(run.passed());
}

#[test]
fn yamllint_runs_when_the_repo_opts_in() {
    if missing("yamllint") {
        return;
    }
    let r = Repo::new();
    r.write(".yamllint", "rules:\n  trailing-spaces: enable\n");
    r.stage("a.yaml", "a: 1   \n"); // trailing spaces
    assert!(!r.hook("pre-commit-yamllint", &[]).passed());
}

// ---- lint-js ------------------------------------------------------------

/// eslint 9+ ERRORS when it finds no config instead of no-op'ing, so the
/// config is the opt-in signal — without it a repo that does not lint JS
/// would fail every commit.
#[test]
fn lint_js_skips_a_repo_with_no_eslint_config() {
    let r = Repo::new();
    r.stage("a.ts", "const x = 1\n");
    let run = r.hook("pre-commit-lint-js", &[]);
    assert!(run.passed());
    assert!(run.says("no eslint config"));
}

#[test]
fn lint_js_reports_a_real_error() {
    if missing("eslint") {
        return;
    }
    let r = Repo::new();
    r.write(
        "eslint.config.js",
        "module.exports = [{rules:{'no-undef':'error'}}];\n",
    );
    r.stage("a.js", "undefinedFunction();\n");
    assert!(!r.hook("pre-commit-lint-js", &[]).passed());
}

// ---- prettier -----------------------------------------------------------

#[test]
fn prettier_does_nothing_without_config_or_a_local_binary() {
    let r = Repo::new();
    r.stage("a.ts", "const  x   =1\n");
    assert!(r.hook("pre-commit-prettier", &[]).passed());
}

#[test]
fn prettier_flags_an_unformatted_file_when_the_repo_opts_in() {
    if missing("prettier") {
        return;
    }
    let r = Repo::new();
    r.write(".prettierrc", "{}\n");
    r.stage("a.ts", "const  x   =1\n");
    assert!(!r.hook("pre-commit-prettier", &[]).passed());

    let r2 = Repo::new();
    r2.write(".prettierrc", "{}\n");
    r2.stage("b.ts", "const x = 1;\n");
    assert!(r2.hook("pre-commit-prettier", &[]).passed());
}

// ---- ruff / pyright -----------------------------------------------------

#[test]
fn ruff_skips_a_repo_with_no_ruff_config() {
    let r = Repo::new();
    r.stage("a.py", "import os\n");
    assert!(r.hook("pre-commit-ruff", &[]).passed());
}

#[test]
fn ruff_reports_lint_and_format_problems_when_the_repo_opts_in() {
    if missing("ruff") && missing("uvx") {
        return;
    }
    let r = Repo::new();
    r.write("pyproject.toml", "[tool.ruff]\n");
    r.stage("a.py", "import os\n"); // unused import
    assert!(!r.hook("pre-commit-ruff", &[]).passed());
}

#[test]
fn pyright_skips_a_repo_with_no_pyright_config() {
    let r = Repo::new();
    r.stage("a.py", "x: int = 'nope'\n");
    assert!(r.hook("pre-commit-pyright", &[]).passed());
}
