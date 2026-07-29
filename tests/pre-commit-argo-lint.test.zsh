#!/bin/zsh
# argo-lint had no tests. These cover the logic that is the hook's OWN — path
# scoping, the Argo-kind filter, and the soft tool gate — none of which needs
# the argo CLI present. What argo itself reports is argo's business.
TEST_NAME=`basename "$0"`
HOOK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test@@'`
git config user.email t@t.test; git config user.name t

stage() { mkdir -p "$(dirname "$1")"; printf '%b' "$2" > "$1"; git add "$1"; }

printf "Should ignore YAML outside the k8s-ish path prefixes\n"
stage src/config.yaml 'kind: Workflow\n'
$HOOK > /tmp/argo-out.txt 2>&1 || exit 1
[ -s /tmp/argo-out.txt ] && { echo "  expected silence, got: $(cat /tmp/argo-out.txt)"; exit 1 }
git rm -q -f --cached src/config.yaml

printf "Should ignore k8s YAML that is not an Argo workflow kind\n"
stage kubernetes/app/deploy.yaml 'kind: Deployment\n'
$HOOK > /tmp/argo-out.txt 2>&1 || exit 1
[ -s /tmp/argo-out.txt ] && { echo "  expected silence, got: $(cat /tmp/argo-out.txt)"; exit 1 }
git rm -q -f --cached kubernetes/app/deploy.yaml

# Each of the four Argo kinds must be recognised. With argo absent the hook
# warns and exits 0; with argo present it lints. Either way it must NOT be
# silent — silence would mean the kind filter missed the file.
printf "Should recognise every Argo workflow kind\n"
for kind in Workflow CronWorkflow WorkflowTemplate ClusterWorkflowTemplate; do
    stage kubernetes/wf/$kind.yaml "kind: $kind\n"
    $HOOK > /tmp/argo-out.txt 2>&1
    [ -s /tmp/argo-out.txt ] || { echo "  $kind was not picked up"; exit 1 }
    git rm -q -f --cached kubernetes/wf/$kind.yaml
done

printf "Should soft-fail (exit 0) when the argo CLI is missing\n"
stage kubernetes/wf/w.yaml 'kind: Workflow\n'
if ! type argo > /dev/null 2>&1; then
    $HOOK > /tmp/argo-out.txt 2>&1 || exit 1     # must not block the commit
    grep -qi "install" /tmp/argo-out.txt || { echo "  no install hint"; exit 1 }
fi
git rm -q -f --cached kubernetes/wf/w.yaml
exit 0
