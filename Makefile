# Git-templates Makefile
# Author: https://github.com/fredericrous
MAKEFILE_PATH      := $(abspath $(lastword $(MAKEFILE_LIST)))
MAKEFILE_DIR       := $(dir $(MAKEFILE_PATH))
XDG_CONFIG_HOME	   ?= ~/.config
HOME_PATH_HOOKS	   := $(XDG_CONFIG_HOME)/git/git-templates/templates/hooks/
SRC_CTRL_HOOKS     := $(MAKEFILE_DIR)templates/hooks/*
GIT_REPO_HOOK_PATH := $(shell git rev-parse --git-dir)/hooks/

all: test

# +x only the scripts, never data files like package.json. Keyed on the
# shebang rather than the extension, so the extensionless git entrypoints
# (commit-msg, pre-commit, pre-push, prepare-commit-msg) get +x while
# package.json (no shebang) is left untouched. POSIX sh — no zsh needed.
chmodx:
	@for f in $(MAKEFILE_DIR)/tests/* $(SRC_CTRL_HOOKS); do \
		if head -1 "$$f" | grep -q '^#!'; then chmod +x "$$f"; fi; \
	done

test: chmodx
	@$(MAKEFILE_DIR)/tests/tests-runner.zsh $(RUN)

# Resolve $(HOME_PATH_HOOKS) and the in-repo source with `pwd -P` before
# touching anything. If both canonicalise to the same directory (the
# common case when ~/.config/git/git-templates is a symlink to this
# working tree, or vice-versa), the original `rm $(HOME_PATH_HOOKS)*`
# wiped the source — skip the rm+cp in that case and only re-init the
# per-repo .git/hooks/. Otherwise behave as before.
install: chmodx
	@mkdir -p $(HOME_PATH_HOOKS)
	@HOME_REAL="$$(cd $(HOME_PATH_HOOKS) && pwd -P)"; \
	SRC_REAL="$$(cd $(MAKEFILE_DIR)templates/hooks/ && pwd -P)"; \
	if [ "$$HOME_REAL" = "$$SRC_REAL" ]; then \
		echo "$(HOME_PATH_HOOKS) resolves to the in-repo source — skipping copy"; \
	else \
		rm -v $(HOME_PATH_HOOKS)* 2>/dev/null || true; \
		cp $(SRC_CTRL_HOOKS) $(HOME_PATH_HOOKS); \
	fi
	@rm -v $(GIT_REPO_HOOK_PATH)* 2>/dev/null || true
	@git init

.PHONY: all chmodx test install
