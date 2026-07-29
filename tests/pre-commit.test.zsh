#!/bin/zsh
# The pre-commit DISPATCHER. Untested until now — every other suite invokes a
# sub-hook directly, so nothing covered the thing that decides which sub-hooks
# run, in what order, and how failures are reported.
#
# The dispatcher globs its OWN directory, so each case builds a throwaway hooks
# dir with fake sub-hooks and runs the shim from there.
TEST_NAME=`basename "$0"`
SHIM=${0:a:h}/../templates/hooks/pre-commit

# Fresh hooks dir with the real shim in it. $1 = dir name.
new_hooks_dir() {
    rm -rf "$1"; mkdir -p "$1"
    cp "$SHIM" "$1/pre-commit"
    chmod +x "$1/pre-commit"
}

printf "Should exit 0 when there are no sub-hooks\n"
new_hooks_dir h_empty
./h_empty/pre-commit || exit 1

printf "Should run every sub-hook\n"
new_hooks_dir h_all
for n in a b c; do
    printf '#!/bin/sh\necho %s >> "$PWD/ran.txt"\n' "$n" > h_all/pre-commit-$n
    chmod +x h_all/pre-commit-$n
done
rm -f ran.txt
./h_all/pre-commit || exit 1
[ "`sort ran.txt | tr -d '\n'`" = "abc" ] || { echo "  got: `cat ran.txt`"; exit 1 }

# Parallelism is the point — serial would be a visible slowdown on EVERY commit.
#
# A rendezvous, NOT a wall-clock threshold. The first version of this case timed
# three 1s sleeps and asserted "under 2s", which failed once on the cold first
# exec of a freshly built binary and passed thereafter — a flaky test guarding
# against flakiness. Here each sub-hook announces itself and then waits for all
# three announcements: under true parallelism they release each other almost
# instantly; if they run serially the first can never see the other two and
# times out. Deterministic, and independent of how fast the machine is.
printf "Should run sub-hooks in parallel, not serially\n"
new_hooks_dir h_par
rm -f rendezvous.txt
for n in a b c; do
    cat > h_par/pre-commit-$n <<'SUB'
#!/bin/sh
echo here >> "$PWD/rendezvous.txt"
i=0
while [ "$(wc -l < "$PWD/rendezvous.txt")" -lt 3 ] && [ $i -lt 50 ]; do
    sleep 0.1
    i=$((i + 1))
done
[ "$(wc -l < "$PWD/rendezvous.txt")" -ge 3 ]
SUB
    chmod +x h_par/pre-commit-$n
done
./h_par/pre-commit || { echo "  sub-hooks never met — they ran serially"; exit 1 }

# Fixing one lint error, committing, and immediately meeting the next is the
# behaviour this prevents. All failures must be reported in one run.
printf "Should report EVERY failure, not just the first\n"
new_hooks_dir h_fail
printf '#!/bin/sh\nexit 1\n' > h_fail/pre-commit-alpha
printf '#!/bin/sh\nexit 0\n' > h_fail/pre-commit-beta
printf '#!/bin/sh\nexit 1\n' > h_fail/pre-commit-gamma
chmod +x h_fail/pre-commit-*
OUT=`./h_fail/pre-commit 2>&1` && exit 1   # must fail
echo "$OUT" | grep -q "pre-commit-alpha" || { echo "  alpha not named"; exit 1 }
echo "$OUT" | grep -q "pre-commit-gamma" || { echo "  gamma not named"; exit 1 }
echo "$OUT" | grep -q "pre-commit-beta"  && { echo "  beta wrongly named"; exit 1 }

printf "Should honour hook.skip as a SUBSTRING match\n"
new_hooks_dir h_skip
printf '#!/bin/sh\necho kept >> "$PWD/skip.txt"\n' > h_skip/pre-commit-kept
printf '#!/bin/sh\necho gone >> "$PWD/skip.txt"\n' > h_skip/pre-commit-package-lock
chmod +x h_skip/pre-commit-*
rm -f skip.txt
git -c hook.skip=package-lock rev-parse > /dev/null   # config is read from the repo
git config --add hook.skip package-lock
./h_skip/pre-commit || exit 1
git config --unset-all hook.skip
[ "`cat skip.txt`" = "kept" ] || { echo "  got: `cat skip.txt`"; exit 1 }

# The zsh dispatcher exited 0 the moment .git/CHERRY_PICK_HEAD existed, so a
# cherry-pick isn't blocked by hooks meant for authored commits.
printf "Should skip everything during a cherry-pick\n"
mkdir -p h_cherry_parent/hooks
cp "$SHIM" h_cherry_parent/hooks/pre-commit
chmod +x h_cherry_parent/hooks/pre-commit
printf '#!/bin/sh\nexit 1\n' > h_cherry_parent/hooks/pre-commit-fails
chmod +x h_cherry_parent/hooks/pre-commit-fails
./h_cherry_parent/hooks/pre-commit && exit 1          # no marker → fails
touch h_cherry_parent/CHERRY_PICK_HEAD
./h_cherry_parent/hooks/pre-commit || exit 1          # marker → skipped

printf "Should pass its arguments through to sub-hooks\n"
new_hooks_dir h_args
printf '#!/bin/sh\necho "$@" > "$PWD/args.txt"\n' > h_args/pre-commit-echo
chmod +x h_args/pre-commit-echo
./h_args/pre-commit one two || exit 1
[ "`cat args.txt`" = "one two" ] || { echo "  got: `cat args.txt`"; exit 1 }

printf "Should fail loudly when the binary cannot be found\n"
new_hooks_dir h_nobin
OUT=`HOME=/nonexistent GIT_HOOKS_BIN=/nonexistent PATH=/nonexistent ./h_nobin/pre-commit 2>&1` && exit 1
echo "$OUT" | grep -q "not found" || { echo "  no clear message: $OUT"; exit 1 }

exit 0
