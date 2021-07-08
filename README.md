# Git Templates with hooks

This is a WIP repository, all hooks might not work, if there is an issue with a hook, just remove it from your '.git/hooks' folder. If you need to push and a hook is preventing you, you could append the option `--no-verify`

## Setup

Clone the repository to a convenient place:

```sh
git clone https://github.com/fredericrousgit-templates.git ~/.config/git/templates
```

Setup your gitconfig

```sh
git config --global init.templatedir ~/.config/git/templates
git config --global commit.template ~/.config/git/message
```

Copy the hooks to existing repositories

```sh
cp ~/.config/git/templates/hooks/* .git/hooks/
```

## Requirements

- NodeJS (minimum v6)
- ZSH
- [ripgrep](https://github.com/BurntSushi/ripgrep/)
## Hooks implemented

| hook | description |
| | |
| commit-msg | Lint commit message to conform to [git recommentation](https://git-scm.com/docs/git-commit) and mostly to[conventional commits]( https://www.conventionalcommits.org/en/v1.0.0/) specification<br>Prefix with an emoji conform to [gitmoji](https://gitmoji.dev/) specification<br>Apply formating to the commit: break the body to 72 char per line, group footers 
| pre-commit-ban-terms | prevent the commit of certain tokens |
| pre-commit-lint | eslint js files in staging |
| pre-commit-package-lock | when a package.json is changed, check package-lock.json is versioned as well 
| pre-push-branch-pattern | insure a branch follows pattern `^(feat|fix|hotfix|chore|test|automation)/\d+-[\w-]+$` |
| pre-push-force-same-branch | should test that when you do a force push, your target the same remote branch and avoid the default one |
| pre-push-pull-rebase | pull the origin remote branch with the same name before push. Fetch default branch and warn if it's ahead |

## Other idea of hook not implemented

- pre-push: when a tag is semver, check that it's defined in a package.json
- pre-commit: lint more languages
- pre-commit: check other languages Gemfile.lock, Pipfile.lock, Cargo.lock
- pre-push: check branch pattern for jira
- repare-commit-msg: extract kabanize or jira id from branch name and prepare a commit msg with a footer Issues: (#id)? <id>
- pre-push: prevent force push to a remote branch that has a different name

## Similar projects

- https://github.com/okonet/lint-staged
- https://github.com/folke/devmoji
- https://github.com/negokaz/git-fancy-message-prefix
- https://github.com/conventional-changelog/commitlint

Some known hook managers:

- https://github.com/sds/overcommit
- https://github.com/evilmartians/lefthook
- https://github.com/pre-commit/pre-commit
- https://github.com/typicode/husky

This project is not meant to be a hook manager. The hooks here tend to be simple.
There is no language restriction. What is important is that the hook is easy to understand and modify
