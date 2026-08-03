#!/bin/sh
# Builds the synthetic fleet the dashboard recording is made against.
# Everything on screen is REAL: real repositories, real shims written by the
# real release binaries, scanned by the real dashboard — only the repositories
# are small and named for a story. Nothing is re-enacted or edited afterwards,
# same rule as demo.sh.
#
# To re-record (needs `asciinema`, `agg`, and the `expect` every macOS ships):
#
#     cargo build --release --bin githooks --bin githooks-fleet
#     sh docs/assets/fleet-demo.sh                # builds /tmp/fleet-root
#     HOME=/tmp/fleet-home XDG_CONFIG_HOME=/tmp/fleet-home/.config \
#     GIT_CONFIG_GLOBAL=/tmp/fleet-home/.gitconfig GIT_CONFIG_SYSTEM=/dev/null \
#         asciinema rec --overwrite --cols 96 --rows 30 \
#         --command "expect docs/assets/fleet-demo.exp" \
#         docs/assets/fleet-demo.cast
#     agg --theme monokai --font-size 15 --idle-time-limit 2 \
#         docs/assets/fleet-demo.cast docs/assets/fleet-demo.gif
#
# The isolated HOME/GIT_CONFIG_* is the same argument as demo.sh: without it
# the recording publishes the recorder's own directory layout and global git
# config to strangers.
#
# expect stands in for the keyboard because a TUI cannot be driven from a
# pipe: it allocates the pty, sends the keystrokes on a schedule, and passes
# the frames through to asciinema untouched.
set -eu

REPO_ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BIN_DIR="$REPO_ROOT/target/release"

export HOME=/tmp/fleet-home
export XDG_CONFIG_HOME=/tmp/fleet-home/.config
export GIT_CONFIG_GLOBAL=/tmp/fleet-home/.gitconfig
export GIT_CONFIG_SYSTEM=/dev/null

rm -rf /tmp/fleet-root /tmp/fleet-home
mkdir -p /tmp/fleet-root "$HOME/.local/bin"
cp "$BIN_DIR/githooks" "$HOME/.local/bin/githooks"
git config --global user.email dev@example.com
git config --global user.name Dev

# A believable spread: each repository carries the files its checks key on.
mk() {
    name=$1
    mkdir -p "/tmp/fleet-root/$name"
    cd "/tmp/fleet-root/$name"
    git init -q -b main .
}

mk shop
printf 'export const cart = [];\n' > cart.js
printf '{}\n' > .prettierrc.json

mk api
printf 'def handler():\n    return 200\n' > app.py
printf '[lint]\nselect = ["E", "F"]\n' > ruff.toml

mk infra
printf 'apiVersion: v1\nkind: Namespace\nmetadata:\n  name: infra\n' > ns.yaml

mk cli
mkdir -p src
printf '[package]\nname = "cli"\nversion = "0.1.0"\nedition = "2021"\n' > Cargo.toml
printf 'fn main() {}\n' > src/main.rs

mk web
printf 'export default {};\n' > index.js
printf '{}\n' > .prettierrc.json

mk blog
printf '# blog\n' > README.md

mk data
printf '{"rows": []}\n' > rows.json

for r in shop api infra cli web blog data; do
    (cd "/tmp/fleet-root/$r" && git add -A && git commit -qm "chore: init")
done

# Shims into every repository, from the fleet binary itself.
"$BIN_DIR/githooks-fleet" install --root /tmp/fleet-root --depth 2 \
    --binary "$HOME/.local/bin/githooks" < /dev/null

# One repository opted a check out — the dashboard announces it rather than
# letting the line rot in .git/config unread.
(cd /tmp/fleet-root/web && git config --add hook.skip pre-commit-prettier)

# And one repository nobody ran install in, because every real fleet has one.
mk legacy
printf '# legacy\n' > README.md
git add -A && git commit -qm "chore: init"

echo "fleet ready at /tmp/fleet-root — now record (see header)"
