#!/bin/zsh
# Prettier --check on staged files, scoped to repos that actually use
# prettier. Catches the formatting issues that a CI `prettier --check`
# step would otherwise only surface after a push.
#
# Scoping: only runs when the repo opts into prettier — either a
# prettier config exists at the git root (.prettierrc*, prettier.config.*,
# or a "prettier" key in package.json) OR a local prettier binary is
# installed. Without that signal we warn+skip, so this never fires in
# repos that don't use prettier (where `npx prettier` would download it
# and flag every file against defaults).
#
# Uses the project-local prettier when present (correct version +
# honours the repo's .prettierignore / config); falls back to npx.
# Author: https://github.com/fredericrous
ERROR_SIGN=$'  \e[38;5;160m✗\e[0m'
VALID_SIGN=$'  \e[38;5;112m✓\e[0m'
WARNING_SIGN=$'  \e[38;5;208m!\e[0m'

# Extensions prettier handles in a typical web project.
EXT_RE='\.(js|jsx|ts|tsx|mjs|cjs|json|jsonc|css|scss|less|html|vue|md|mdx|ya?ml)$'

staged=(${(f)"$(git diff --diff-filter=d --cached --name-only)"})
(( ${#staged} == 0 )) && exit 0
files=(${(f)"$(printf '%s\n' $staged | grep -E $EXT_RE)"})
(( ${#files} == 0 )) && exit 0

ROOT=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0

# --- decide whether this repo opts into prettier ---------------------
# (N) = null-glob: unmatched patterns expand to nothing instead of
# erroring under zsh's default NOMATCH. (.) = plain files only.
has_config=0
config_matches=("$ROOT"/.prettierrc(N.) "$ROOT"/.prettierrc.*(N.) "$ROOT"/prettier.config.*(N.))
(( ${#config_matches} > 0 )) && has_config=1
if (( ! has_config )) && [[ -f "$ROOT/package.json" ]]; then
    if type node > /dev/null 2>&1; then
        node -e 'process.exit(JSON.parse(require("fs").readFileSync(process.argv[1],"utf8")).prettier?0:1)' \
            "$ROOT/package.json" 2>/dev/null && has_config=1
    fi
fi

# --- resolve a prettier binary ---------------------------------------
local_bin=""
[[ -x "$ROOT/node_modules/.bin/prettier" ]] && local_bin="$ROOT/node_modules/.bin/prettier"

if (( ! has_config )) && [[ -z "$local_bin" ]]; then
    # Repo doesn't use prettier — nothing to enforce.
    exit 0
fi

if [[ -n "$local_bin" ]]; then
    PRETTIER=("$local_bin")
elif type prettier > /dev/null 2>&1; then
    PRETTIER=(prettier)
elif type npx > /dev/null 2>&1; then
    PRETTIER=(npx --no-install prettier)
    # --no-install: don't silently download into an unrelated repo. If
    # it's not resolvable, warn+skip rather than block.
    if ! npx --no-install prettier --version > /dev/null 2>&1; then
        printf "$WARNING_SIGN prettier config found but no prettier binary. Run \033[38;5;208mnpm install\033[0m\n"
        exit 0
    fi
else
    printf "$WARNING_SIGN Staged files prettier handles, but neither prettier nor npx is available.\n"
    exit 0
fi

# --check respects the repo's config + .prettierignore automatically.
if ! "${PRETTIER[@]}" --check $files > /dev/null 2>&1; then
    printf "$ERROR_SIGN Prettier found unformatted files. Run \033[38;5;208mprettier --write\033[0m on:\n"
    "${PRETTIER[@]}" --list-different $files 2>/dev/null | sed 's/^/      /'
    exit 1
fi
printf "$VALID_SIGN Prettier passed\n"
