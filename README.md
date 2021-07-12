# Git Templates with hooks

Git Starter Template with opinionated hooks to help you create beautiful commits with high quality standards..and emojis ✨

<img src="https://user-images.githubusercontent.com/702227/125003867-1b012f00-e050-11eb-8641-748ef806c639.png" width="800">

*If there is an issue with a hook, please open an issue and consult the section [Opt Out](https://github.com/fredericrous/git-templates/wiki/Opt-Out) for a workaround.*

## The workflow

First, `commit`. A nice template [message](https://github.com/fredericrous/git-templates/blob/main/message) appears to help you write a meaningful commit description that passes the requirements.
The message saved, [validators](https://github.com/fredericrous/git-templates/wiki/Hooks-implemented) run in parallel. If there is an issue, the commit is aborted
Ready to push? once `git push` is started, the tests runs for the module you updated, branch name is checked. The branch is pushed. Bravo

The wiki explore in details [this workflow](https://github.com/fredericrous/git-templates/wiki/Coding-Flow)

The wiki also lists all the [implemented hooks](https://github.com/fredericrous/git-templates/wiki/Hooks-implemented)

## Setup

Clone the repository to a convenient place:

```sh
mkdir ~/.config/git
cd ~/.config/git
git clone https://github.com/fredericrous/git-templates.git
chmod +x templates/hooks/*
```

Setup your gitconfig

```sh
git config --global init.templatedir ~/.config/git/git-templates/templates
git config --global commit.template ~/.config/git/git-templates/message
```

Copy the hooks to existing repositories

```sh
cd <folder-of-your-repo>
git init
```

## Update

Update the local clone to the latest version

```sh
cd ~/.config/git/git-templates/templates
git pull
```

Update the target repository

```sh
rm $(git rev-parse --git-dir)/hooks/*
git init
```

## Requirements

- Git 2.22+
- ZSH
- NodeJS 11.7+
- [ripgrep](https://github.com/BurntSushi/ripgrep/)

## Wiki

- [Coding Flow](https://github.com/fredericrous/git-templates/wiki/Coding-Flow) - an explanation of where the hooks fit in your git "flow"
- [Commit Prefix](https://github.com/fredericrous/git-templates/wiki/Commit-Prefix) - list of prefix your commit summaries should contain
- [Hooks Implemented](https://github.com/fredericrous/git-templates/wiki/Hooks-implemented) - all the hooks that are triggered when you execute a git command
- [Ideas of hooks to implement](https://github.com/fredericrous/git-templates/wiki/Ideas-of-hooks-to-implement) - a list of ideas, not a roadmap
- [Opt Out](https://github.com/fredericrous/git-templates/wiki/Opt-Out) - bypass a check, a hook or uninstall it
- [Similar Projects](https://github.com/fredericrous/git-templates/wiki/Similar-projects)

## Contribute

Basically if a script is simple implement it in shell script. If the logic is complicated, use javascript or any proper language to implement it. Javascript is nice because nowadays a lot of devs have nodejs installed on their machine.

There's a makefile, open it, see the different tasks, basically:

- `make test` runs the tests
- `make` is an alias to `make test`
- `make install` copies the hooks from this repo to both .git/ and ~/.config/git/

To run only one test, use `make test RUN=<part of the name of the test>`
