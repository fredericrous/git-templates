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

# GIT_HOOKS_BIN points the shims at the freshly built debug binary. Without it
# they would fall through to the unsubstituted @BIN@ placeholder (this is the
# SOURCE tree — @BIN@ is only filled in at install time) and then to PATH, where
# a stale installed binary could silently answer instead of the one under test.
test: chmodx build
	@GIT_HOOKS_BIN=$(DEBUG_BIN) $(MAKEFILE_DIR)/tests/tests-runner.zsh $(RUN)

# Resolve $(HOME_PATH_HOOKS) and the in-repo source with `pwd -P` before
# touching anything. If both canonicalise to the same directory (the
# common case when ~/.config/git/git-templates is a symlink to this
# working tree, or vice-versa), the original `rm $(HOME_PATH_HOOKS)*`
# wiped the source — skip the rm+cp in that case and only re-init the
# per-repo .git/hooks/. Otherwise behave as before.
install: chmodx
	@cargo build --release --quiet
	@mkdir -p $(INSTALL_BIN_DIR)
	@install -m 0755 $(RELEASE_BIN) $(INSTALLED_BIN)
	@echo "installed $(INSTALLED_BIN)"
	@mkdir -p $(HOME_PATH_HOOKS)
	@HOME_REAL="$$(cd $(HOME_PATH_HOOKS) && pwd -P)"; \
	SRC_REAL="$$(cd $(MAKEFILE_DIR)templates/hooks/ && pwd -P)"; \
	if [ "$$HOME_REAL" = "$$SRC_REAL" ]; then \
		echo "$(HOME_PATH_HOOKS) resolves to the in-repo source — skipping copy"; \
	else \
		rm -v $(HOME_PATH_HOOKS)* 2>/dev/null || true; \
		cp $(SRC_CTRL_HOOKS) $(HOME_PATH_HOOKS); \
	fi
	@$(MAKE) --no-print-directory bake-shims DIR=$(HOME_PATH_HOOKS)
	@rm -v $(GIT_REPO_HOOK_PATH)* 2>/dev/null || true
	@git init
	@$(MAKE) --no-print-directory bake-shims DIR=$(GIT_REPO_HOOK_PATH)

# Replace the @BIN@ placeholder with the absolute installed path. This is the
# whole reason GUI git clients work: they launch git without the PATH that a
# login shell would have, so a shim relying on PATH alone resolves nothing and
# the hook fails. Idempotent — re-baking an already-baked shim is a no-op
# because the placeholder is gone.
bake-shims:
	@for f in $(DIR)pre-commit $(DIR)pre-push; do \
		[ -f "$$f" ] || continue; \
		if grep -q '@BIN@' "$$f" 2>/dev/null; then \
			sed "s|@BIN@|$(INSTALLED_BIN)|g" "$$f" > "$$f.tmp" && mv "$$f.tmp" "$$f" && chmod +x "$$f"; \
			echo "  baked $$f -> $(INSTALLED_BIN)"; \
		fi; \
	done

.PHONY: all chmodx build test install bake-shims
