#!/bin/zsh
TEST_NAME=`basename "$0"`
HOOK_CHECK=`echo ${0:a:h}/../templates/hooks/$TEST_NAME | sed 's@\.test@@'`

# The hook only runs when the repo opts into pyright. Give the temp test
# repo a [tool.pyright] config so the gate passes.
cat > pyproject.toml <<'EOF'
[tool.pyright]
typeCheckingMode = "basic"
EOF

# Resolve pyright the same way the hook does; skip gracefully if none is
# available (the hook itself warn+skips, so assertions would be moot).
# uvx pyright cold-starts a bundled node on first run, so the probe also
# warms it before the timed assertions.
pyright_ok=0
if [[ -x ".venv/bin/pyright" ]] || type pyright > /dev/null 2>&1; then
    pyright_ok=1
elif type uvx > /dev/null 2>&1 && uvx pyright --version > /dev/null 2>&1; then
    pyright_ok=1
fi
(( ! pyright_ok )) && { printf "  ! pyright/uvx unavailable — skipping\n"; exit 0 }

printf "Should pass on a well-typed file\n"
TEST_FILE="ok.py"
printf 'x: int = 1\n' > $TEST_FILE
git add $TEST_FILE
$HOOK_CHECK &> /dev/null || exit 1

printf "Should throw on a type error\n"
# str assigned to an int-annotated name -> reportAssignmentType, an error
# in both basic and strict modes -> pyright exits non-zero.
printf 'x: int = "not an int"\n' > $TEST_FILE
git add $TEST_FILE
$HOOK_CHECK &> /dev/null && exit 1

printf "Should skip when the repo has no pyright config\n"
git reset -q
rm -f $TEST_FILE pyproject.toml
printf 'x: int = "not an int"\n' > $TEST_FILE
git add $TEST_FILE
# No [tool.pyright] / pyrightconfig -> opt-in gate fails -> hook exits 0
# even though the file has a type error.
$HOOK_CHECK &> /dev/null || exit 1

printf "Should ignore non-Python files\n"
git reset -q
rm -f $TEST_FILE
cat > pyproject.toml <<'EOF'
[tool.pyright]
typeCheckingMode = "basic"
EOF
TEST_FILE="data.txt"
printf 'not python at all\n' > $TEST_FILE
git add $TEST_FILE
# No .py files staged -> hook exits 0 without running pyright.
$HOOK_CHECK &> /dev/null || exit 1

exit 0
