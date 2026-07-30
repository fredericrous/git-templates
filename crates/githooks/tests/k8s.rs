//! The three Kubernetes hooks. Each covers its OWN logic — path scoping, kind
//! filtering, config and tool gates, kustomization-root discovery — not what
//! the external tools report, which is their business.

mod common;
use common::{missing, Repo};

const DEPLOY: &str = "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: d\n";

// ---- argo-lint ----------------------------------------------------------

#[test]
fn argo_ignores_yaml_outside_the_k8s_prefixes() {
    let r = Repo::new();
    r.stage("src/config.yaml", "kind: Workflow\n");
    assert!(r.hook("pre-commit-argo-lint", &[]).silent());
}

#[test]
fn argo_ignores_k8s_yaml_that_is_not_a_workflow() {
    let r = Repo::new();
    r.stage("kubernetes/app/deploy.yaml", DEPLOY);
    assert!(r.hook("pre-commit-argo-lint", &[]).silent());
}

/// Every Argo kind must be picked up. With `argo` absent the hook warns and
/// exits 0; with it present it lints. Either way it must NOT be silent —
/// silence would mean the kind filter missed the file.
#[test]
fn argo_recognises_every_workflow_kind() {
    for kind in [
        "Workflow",
        "CronWorkflow",
        "WorkflowTemplate",
        "ClusterWorkflowTemplate",
    ] {
        let r = Repo::new();
        r.stage(
            &format!("kubernetes/wf/{kind}.yaml"),
            &format!("kind: {kind}\n"),
        );
        assert!(
            !r.hook("pre-commit-argo-lint", &[]).silent(),
            "{kind} was not picked up"
        );
    }
}

#[test]
fn argo_soft_fails_without_the_cli() {
    if !missing("argo") {
        return; // the gate only exists when the tool is absent
    }
    let r = Repo::new();
    r.stage("kubernetes/wf/w.yaml", "kind: Workflow\n");
    let run = r.hook("pre-commit-argo-lint", &[]);
    assert!(run.passed(), "a missing toolchain must not block a commit");
    assert!(run.says("install"));
}

// ---- kube-linter --------------------------------------------------------

#[test]
fn kube_linter_ignores_yaml_outside_the_k8s_prefixes() {
    let r = Repo::new();
    r.stage("src/app.yaml", DEPLOY);
    assert!(r.hook("pre-commit-kube-linter", &[]).silent());
}

/// Stock rules are too noisy to enforce generically, so a repo-local config is
/// the opt-in. Without one the hook must SAY it skipped, not lint anyway.
#[test]
fn kube_linter_says_it_skipped_without_a_config() {
    let r = Repo::new();
    r.stage("kubernetes/app/deploy.yaml", DEPLOY);
    let run = r.hook("pre-commit-kube-linter", &[]);
    assert!(run.passed());
    assert!(run.says("skipping") || run.says("install"));
}

// ---- kubeconform --------------------------------------------------------

#[test]
fn kubeconform_ignores_yaml_outside_the_k8s_prefixes() {
    let r = Repo::new();
    r.stage("src/app.yaml", DEPLOY);
    assert!(r.hook("pre-commit-kubeconform", &[]).silent());
}

/// Raw-YAML validation is out of scope: a project either uses kustomize or it
/// does not. With no kustomization above the file there is nothing to do.
#[test]
fn kubeconform_is_silent_with_no_kustomization_root() {
    if missing("kustomize") || missing("kubeconform") {
        return; // the tool gate runs BEFORE root discovery
    }
    let r = Repo::new();
    r.stage("kubernetes/loose/deploy.yaml", DEPLOY);
    assert!(r.hook("pre-commit-kubeconform", &[]).silent());
}

/// The walk-up: a kustomization ABOVE the staged file must be found.
#[test]
fn kubeconform_finds_a_root_above_the_staged_file() {
    let r = Repo::new();
    r.stage(
        "kubernetes/app/base/kustomization.yaml",
        "resources:\n  - deploy.yaml\n",
    );
    r.stage("kubernetes/app/base/deploy.yaml", DEPLOY);
    assert!(
        !r.hook("pre-commit-kubeconform", &[]).silent(),
        "the root above the file was not discovered"
    );
}
