#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test@@'`

# The hook only runs when the repo opts into ruff. Give the temp test
# repo a [tool.ruff] config so the gate passes; default lint select
# (E4/E7/E9/F) + the formatter are enough to tell the cases apart.
cat > pyproject.toml <<'EOF'
[tool.ruff]
line-length = 100
EOF

# Resolve ruff the same way the hook does; skip gracefully if none is
# available (the hook itself warn+skips, so assertions would be moot).
# Mirrors the npx-availability probe in the prettier test.
ruff_ok=0
if [[ -x ".venv/bin/ruff" ]] || type ruff > /dev/null 2>&1; then
    ruff_ok=1
elif type uvx > /dev/null 2>&1 && uvx ruff --version > /dev/null 2>&1; then
    ruff_ok=1
fi
(( ! ruff_ok )) && { printf "  ! ruff/uvx unavailable — skipping\n"; exit 0 }

printf "Should pass on a clean, well-formatted file\n"
TEST_FILE="ok.py"
printf 'a = 1\n' > $TEST_FILE
git add $TEST_FILE
$HOOK_CHECK &> /dev/null || exit 1

printf "Should throw on a badly-formatted file (format --check)\n"
# `a  =  1` is format-dirty (the formatter rewrites it) but NOT in the
# default lint select — so this isolates the `ruff format --check` pass,
# the exact step a `ruff check`-only hook would miss.
printf 'a  =  1\n' > $TEST_FILE
git add $TEST_FILE
$HOOK_CHECK &> /dev/null && exit 1

printf "Should throw on a lint error (check)\n"
# Unused import -> F401 (in the default select). Format-clean, so this
# isolates the `ruff check` pass.
printf 'import os\n' > $TEST_FILE
git add $TEST_FILE
$HOOK_CHECK &> /dev/null && exit 1

printf "Should skip when the repo has no ruff config\n"
git reset -q
rm -f $TEST_FILE pyproject.toml
printf 'a  =  1\n' > $TEST_FILE
git add $TEST_FILE
# No [tool.ruff] / ruff.toml -> opt-in gate fails -> hook exits 0 even
# though the file is badly formatted.
$HOOK_CHECK &> /dev/null || exit 1

printf "Should ignore non-Python files\n"
git reset -q
rm -f $TEST_FILE
cat > pyproject.toml <<'EOF'
[tool.ruff]
line-length = 100
EOF
TEST_FILE="data.txt"
printf 'not   python   at all\n' > $TEST_FILE
git add $TEST_FILE
# No .py files staged -> hook exits 0 without running ruff.
$HOOK_CHECK &> /dev/null || exit 1

exit 0
