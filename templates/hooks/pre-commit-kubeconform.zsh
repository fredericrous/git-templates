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

# Optional repo-local skip list: a `.kubeconform-skip` file at the repo root,
# one Kind per line (`#` comments allowed). Use for CRDs whose published JSON
# schema is wrong — e.g. the datreeio cilium schemas type CIDR fields as
# IPv4-only, so any IPv6/dual-stack CiliumEgressGatewayPolicy false-fails.
# These kinds are validated by their operator's admission webhook anyway.
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
          --schema-location 'https://raw.githubusercontent.com/datreeio/CRDs-catalog/main/{{.Group}}/{{.ResourceKind}}_{{.ResourceAPIVersion}}.json' \
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
