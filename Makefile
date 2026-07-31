# Git-templates Makefile
# Author: https://github.com/fredericrous
MAKEFILE_PATH      := $(abspath $(lastword $(MAKEFILE_LIST)))
MAKEFILE_DIR       := $(dir $(MAKEFILE_PATH))
XDG_CONFIG_HOME	   ?= ~/.config
SRC_CTRL_HOOKS     := $(MAKEFILE_DIR)templates/hooks/*

# Windows produces githooks.exe; everywhere else, githooks. Detected from the
# ARTIFACT rather than by sniffing the OS: Git Bash, MSYS2 and Cygwin report
# three different `uname` strings and all build the same .exe. Recursive `=`,
# not `:=`, so it is evaluated when a recipe line runs — i.e. after the
# `cargo build` above it, when the file exists to be detected.
EXE                 = $(shell [ -f "$(MAKEFILE_DIR)target/release/githooks.exe" ] \
                        || [ -f "$(MAKEFILE_DIR)target/debug/githooks.exe" ] && echo .exe)
DEBUG_BIN           = $(MAKEFILE_DIR)target/debug/githooks$(EXE)
RELEASE_BIN         = $(MAKEFILE_DIR)target/release/githooks$(EXE)

all: test

# +x only the scripts, never data files like package.json. Keyed on the
# shebang rather than the extension, so the extensionless git entrypoints
# (commit-msg, pre-commit, pre-push, prepare-commit-msg) get +x while
# package.json (no shebang) is left untouched. POSIX sh — no zsh needed.
chmodx:
	@for f in $(MAKEFILE_DIR)crates/githooks/tests/* $(SRC_CTRL_HOOKS); do \
		[ -f "$$f" ] || continue; \
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
	@./scripts/check-no-deps.sh
	@cargo test $(if $(RUN),--test $(RUN)) -- --show-output

# Installation lives in the BINARY, not here: `githooks install`.
#
# The routine has to decide whether a directory may be emptied —
# ~/.config/git/git-templates is commonly a symlink to a checkout, so
# "installing" there means deleting TRACKED files. That guard was got wrong
# twice. A Makefile can only be tested on the platforms that have `make`, and
# Git for Windows ships bash and coreutils but NOT make, so the shell version
# would have needed a PowerShell twin: two implementations of one dangerous
# guard. It is Rust now, with the worktree case that caused the second incident
# under test on every platform.
install: chmodx
	@cargo build --release --quiet
	@$(RELEASE_BIN) install

# The dashboard, installed SEPARATELY and on purpose.
#
# `make install` ships only the hook binary. That one runs on every commit in
# every repo, and its dependency-free posture is the entire reason the Rust
# migration happened; githooks-fleet pulls ratatui, crossterm and serde. Keeping
# the two installs apart is what stops "I wanted the dashboard" from becoming
# "every commit now depends on a TUI library".
#
# Nothing is deleted here — it writes one binary — so this needs none of the
# fail-closed machinery `install` carries.
install-fleet:
	@cargo build --release --quiet -p githooks-fleet
	@mkdir -p $(INSTALL_BIN_DIR)
	@install -m 0755 $(MAKEFILE_DIR)target/release/githooks-fleet $(INSTALL_BIN_DIR)/githooks-fleet
	@echo "installed $(INSTALL_BIN_DIR)/githooks-fleet"
	@echo "  githooks-fleet          report the fleet"
	@echo "  githooks-fleet tui      the dashboard"
	@echo "  githooks-fleet fix      what would change (dry run; --apply to write)"

# Push the shim SET to every repo. Only needed when a hook is added, removed
# or renamed — ordinary binary fixes reach all repos via `make install`, since
# every shim points at the one binary.
#
# This was a shell script until a Rust plan was proved to remove exactly the
# same set on a fixture covering every actionable case. Dry-run by default;
# APPLY=1 to write.
propagate: 
	@cargo run -q -p githooks-fleet -- fix $(if $(APPLY),--apply,)

# The commit path must stay dependency-free; see the script.
deps:
	@./scripts/check-no-deps.sh

.PHONY: all chmodx build test install install-fleet propagate deps
