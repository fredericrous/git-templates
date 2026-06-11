#!/bin/zsh
# Validate staged Kubernetes manifests with kustomize build | kubeconform.
# Only runs when BOTH tools are installed — otherwise warns and skips so
# the hook doesn't gate on every developer having the toolchain.
# Author: https://github.com/fredericrous
ERROR_SIGN="  [38;5;160m✗[0m"
VALID_SIGN="  [38;5;112m✓[0m"
WARNING_SIGN="  [38;5;208m![0m"

# Trigger only when staged paths look kubernetes-ish — same prefix set as
# pre-commit-kube-linter.zsh.
STAGED=`git diff --diff-filter=d --cached --name-only | grep -E '^(kubernetes|manifests|charts?|k8s|helm|deploy)/.*\.ya?ml$'`
[ ${#STAGED} -lt 1 ] && exit 0

# Both tools required. Name the missing one(s) so the operator knows what
# to install. Soft-fail.
MISSING=()
type kustomize > /dev/null || MISSING+=("kustomize")
type kubeconform > /dev/null || MISSING+=("kubeconform")
if [ ${#MISSING} -gt 0 ]; then
    printf "$WARNING_SIGN Kubernetes manifests staged. Skipping kubeconform; install: [38;5;208m${(j:, :)MISSING}[0m\n"
    exit 0
fi

# Find each kustomization root whose subtree contains a staged file.
# A "kustomization root" is any dir containing kustomization.yaml or .yml.
# We walk up from each staged file's parent until we find one (or hit the
# repo root).
typeset -aU ROOTS
for f in ${(f)STAGED}; do
    dir=$(dirname "$f")
    while [ "$dir" != "." ] && [ "$dir" != "/" ]; do
        if [ -f "$dir/kustomization.yaml" ] || [ -f "$dir/kustomization.yml" ]; then
            ROOTS+=("$dir")
            break
        fi
        dir=$(dirname "$dir")
    done
done

# Nothing under a kustomization root — exit silently (raw-YAML validation
# is out of scope; project either uses kustomize or it doesn't).
[ ${#ROOTS} -lt 1 ] && exit 0

# kubeconform validates CORE Kubernetes kinds against the built-in schema.
# We deliberately do NOT pull an external CRD catalog (the datree/datreeio
# CRDs-catalog is unmaintained — Datree itself is EOL — and its cilium schema
# typed CIDR fields IPv4-only, false-failing IPv6 policies). CRDs are skipped
# here (--ignore-missing-schemas) and covered elsewhere in the stack: Kyverno
# CLI (policy), Trivy (misconfig) and each operator's admission webhook.
#
# Optional escape hatch: a repo-local `.kubeconform-skip` file (one Kind per
# line, `#` comments) force-skips kinds even if a schema is supplied — for the
# day someone vendors local CRD schemas.
SKIP_ARGS=()
if [ -f .kubeconform-skip ]; then
    SKIP_KINDS=$(grep -vE '^\s*(#|$)' .kubeconform-skip | paste -sd, - | sed 's/ //g')
    [ -n "$SKIP_KINDS" ] && SKIP_ARGS=(--skip "$SKIP_KINDS")
fi

OVERALL=0
for root in $ROOTS; do
    # `kustomize build` errors stream to stderr; the pipe still captures
    # stdout. Pipefail catches a build failure even when kubeconform
    # would otherwise consume an empty input cleanly.
    setopt local_options pipefail
    kustomize build "$root" 2>&1 | \
        kubeconform \
          --strict \
          --ignore-missing-schemas \
          --schema-location default \
          $SKIP_ARGS \
          --summary -
    rc=$?
    if [ $rc -ne 0 ]; then
        printf "$ERROR_SIGN kubeconform failed for $root\n"
        OVERALL=1
    fi
done

if [ $OVERALL -ne 0 ]; then
    exit 1
fi
printf "$VALID_SIGN kubeconform passed (${#ROOTS} kustomization root$([ ${#ROOTS} -gt 1 ] && echo 's'))\n"
