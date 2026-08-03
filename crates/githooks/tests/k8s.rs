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
/// the opt-in. Without one the hook does nothing AND SAYS NOTHING.
///
/// CHANGE OF MIND, recorded: this case used to be
/// `kube_linter_says_it_skipped_without_a_config`, and asserted that the notice
/// MUST exist. Two reasons it is now the opposite. `yamllint::run` returns
/// silently in precisely this situation and is the precedent — one of these
/// three checks behaving differently is a difference nobody chose. And the
/// notice fires on EVERY commit touching `kubernetes/**.yaml` in a repository
/// that has no `.kube-linter*.yaml` and never will, which is the same "a repo
/// that never wanted yamllint was told to install it" noise
/// `docs/hook-architecture.md` records these three checks being fixed for.
///
/// The sibling assertion is KEPT: the opt-in is tested before the binary, so a
/// repo with no config is still never asked to install a linter it does not
/// use.
#[test]
fn kube_linter_is_silent_without_a_config() {
    let r = Repo::new();
    r.stage("kubernetes/app/deploy.yaml", DEPLOY);
    let run = r.hook("pre-commit-kube-linter", &[]);
    assert!(run.passed());
    assert!(
        run.silent(),
        "a repo that never opted in must hear nothing:\n{}",
        run.output()
    );
    assert!(
        !run.says("install"),
        "nagged a repo that never opted in:\n{}",
        run.output()
    );
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
///
/// Unconditional: root discovery runs BEFORE the tool gate, so this holds on a
/// machine with neither kustomize nor kubeconform installed — which is most of
/// them, and exactly where a wrongly-ordered gate would have gone unnoticed.
#[test]
fn kubeconform_is_silent_with_no_kustomization_root() {
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
