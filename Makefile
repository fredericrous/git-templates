# Git-templates Makefile
# Author: https://github.com/fredericrous
MAKEFILE_PATH      := $(abspath $(lastword $(MAKEFILE_LIST)))
MAKEFILE_DIR       := $(dir $(MAKEFILE_PATH))
XDG_CONFIG_HOME	   ?= ~/.config
HOME_PATH_HOOKS	   := $(XDG_CONFIG_HOME)/git/git-templates/templates/hooks/
SRC_CTRL_HOOKS     := $(MAKEFILE_DIR)templates/hooks/*
GIT_REPO_HOOK_PATH := $(shell git rev-parse --git-dir)/hooks/

all: test

chmodx:
	@chmod +x $(MAKEFILE_DIR)/tests/* $(SRC_CTRL_HOOKS)

test: chmodx
	@$(MAKEFILE_DIR)/tests/tests-runner.zsh $(RUN)

install: chmodx
	@rm -v $(GIT_REPO_HOOK_PATH)* $(HOME_PATH_HOOKS)*
	cp $(SRC_CTRL_HOOKS) $(HOME_PATH_HOOKS)
	@git init

.PHONY: all chmodx test install
