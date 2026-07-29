# Git-templates Makefile
# Author: https://github.com/fredericrous
MAKEFILE_PATH      := $(abspath $(lastword $(MAKEFILE_LIST)))
MAKEFILE_DIR       := $(dir $(MAKEFILE_PATH))
XDG_CONFIG_HOME	   ?= ~/.config
HOME_PATH_HOOKS	   := $(XDG_CONFIG_HOME)/git/git-templates/templates/hooks/
SRC_CTRL_HOOKS     := $(MAKEFILE_DIR)templates/hooks/*
GIT_REPO_HOOK_PATH := $(shell git rev-parse --git-dir)/hooks/

DEBUG_BIN          := $(MAKEFILE_DIR)target/debug/githooks
RELEASE_BIN        := $(MAKEFILE_DIR)target/release/githooks
INSTALL_BIN_DIR    ?= $(HOME)/.local/bin
INSTALLED_BIN      := $(INSTALL_BIN_DIR)/githooks

all: test

# +x only the scripts, never data files like package.json. Keyed on the
# shebang rather than the extension, so the extensionless git entrypoints
# (commit-msg, pre-commit, pre-push, prepare-commit-msg) get +x while
# package.json (no shebang) is left untouched. POSIX sh — no zsh needed.
chmodx:
	@for f in $(MAKEFILE_DIR)/tests/* $(SRC_CTRL_HOOKS); do \
		if head -1 "$$f" | grep -q '^#!'; then chmod +x "$$f"; fi; \
	done

build:
	@cargo build --quiet

# `--show-output` so a case that SKIPPED for a missing tool is visible: cargo
# hides stdout for passing tests, and a silent skip is indistinguishable from a
# pass — the trap the zsh suites already guarded against.
#
# RUN=<filter> still works: `make test RUN=branch_pattern`.
test: chmodx
	@cargo test $(if $(RUN),--test $(RUN)) -- --show-output

# Never rm/cp/bake into a directory that lives inside a git checkout.
#
# ~/.config/git/git-templates is commonly a SYMLINK to a checkout of this repo,
# in which case "installing" there means deleting and overwriting TRACKED source
# files. Comparing it against $(MAKEFILE_DIR) is not enough: run `make install`
# from a git WORKTREE and the two resolve to different paths — a different
# checkout of the same repo — so the guard says "not the source" and clobbers
# the main checkout. Observed doing exactly that, including baking a
# machine-specific absolute path into tracked files.
#
# Asking git is the reliable test, whatever route the symlink took. When the
# target is inside a checkout there is nothing to install anyway: `git init`
# already reads its templates from there.
install: chmodx
	@cargo build --release --quiet
	@mkdir -p $(INSTALL_BIN_DIR)
	@install -m 0755 $(RELEASE_BIN) $(INSTALLED_BIN)
	@echo "installed $(INSTALLED_BIN)"
	@mkdir -p $(HOME_PATH_HOOKS)
	@HOME_REAL="$$(cd $(HOME_PATH_HOOKS) && pwd -P)"; \
	if git -C "$$HOME_REAL" rev-parse --show-toplevel > /dev/null 2>&1; then \
		echo "$(HOME_PATH_HOOKS) is inside a git checkout — leaving it alone"; \
	else \
		rm -v $(HOME_PATH_HOOKS)* 2>/dev/null || true; \
		cp $(SRC_CTRL_HOOKS) $(HOME_PATH_HOOKS); \
		$(MAKE) --no-print-directory bake-shims DIR=$(HOME_PATH_HOOKS); \
	fi
	@rm -v $(GIT_REPO_HOOK_PATH)* 2>/dev/null || true
	@git init
	@$(MAKE) --no-print-directory bake-shims DIR=$(GIT_REPO_HOOK_PATH)

# Replace the __GITHOOKS_BIN__ placeholder with the absolute installed path,
# in EVERY shim — not a hardcoded pair. It was written when pre-commit and
# pre-push were the only two; five later shims silently never got baked.
# The token is deliberately distinct from anything in the surrounding prose,
# because a global sed on a placeholder that also appears in the comments
# EXPLAINING it rewrites the explanation. This is the
# whole reason GUI git clients work: they launch git without the PATH that a
# login shell would have, so a shim relying on PATH alone resolves nothing and
# the hook fails. Idempotent — re-baking an already-baked shim is a no-op
# because the placeholder is gone.
#
# NEVER call this on the in-repo templates/hooks/. ~/.config/git/git-templates
# is commonly a SYMLINK to this working tree, so baking "the installed copy"
# would rewrite the tracked source with a machine-specific absolute path —
# dirtying the tree and, if committed, shipping one developer's $HOME to all 96
# repos. `install` therefore bakes only real copies: the per-repo .git/hooks/,
# and ~/.config/... only when it is NOT the source.
bake-shims:
	@for f in $(DIR)*; do \
		[ -f "$$f" ] || continue; \
		if grep -q '__GITHOOKS_BIN__' "$$f" 2>/dev/null; then \
			sed "s|__GITHOOKS_BIN__|$(INSTALLED_BIN)|g" "$$f" > "$$f.tmp" && mv "$$f.tmp" "$$f" && chmod +x "$$f"; \
			echo "  baked $$f"; \
		fi; \
	done

.PHONY: all chmodx build test install bake-shims
