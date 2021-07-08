# Git Templates with hooks

Simple hooks for your development workflow.
This repository gives you a started template. It's meant to inspire you to create the hooks that fit you.

*This repository is a WIP. If there is an issue with a hook, please open an issue and Consult the section OPT Out.*

Want to start right away? Check the **Requirements** and head to the section **Setup**.

## The workflow

You are working on a badass feature. Now's the time to share your work.
You hit `git commit`, a nice template `message` appears to help you write a meaningful log description.
Or maybe you are lazy and just write `git commit -m"Add to Cart"`.

## Pre-commit

Git triggers the `pre-commit` hook before the commit actually occurs. The hook calls in parallel all the scripts prefixed with `pre-commit-`. These scripts check the staged files for code that shouldn't be commited as is and more, see section **Hooks implemented**.
If one check fail, the commit is aborted. Otherwise the next hook is triggered, `commit-msg`.

## Commit-msg

This hook lints your message, to help to keep some conventions in place, and it happens an emoji to your commit.
Here are the conventions the hook takes its inspiration from:

- https://git-scm.com/docs/git-commit
- https://www.conventionalcommits.org/en/v1.0.0/
- https://gitmoji.dev/

Once the message complies with the checks, the commit is finally created.

## Pre-push

When you run the command `git push`, the hook `pre-push` gets triggered. All the scripts prefixed with `pre-push` get executed in sequence. See section **Hooks implemented**.

## Requirements

- NodeJS (minimum v6)
- ZSH
- [ripgrep](https://github.com/BurntSushi/ripgrep/)

## Setup

Clone the repository to a convenient place:

```sh
mkdir ~/.config/git
git clone https://github.com/fredericrousgit-templates.git ~/.config/git/templates
chmod +x ~/.config/git/templates/hooks/*
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

## Hooks implemented

| hook | description |
| | |
| commit-msg | Lint commit message to conform to [git recommentation](https://git-scm.com/docs/git-commit) and mostly to[conventional commits]( https://www.conventionalcommits.org/en/v1.0.0/) specification<br>Prefix with an emoji conform to [gitmoji](https://gitmoji.dev/) specification<br>Apply formating to the commit: break the body to 72 char per line. It groups footers. It insures the body is separated by a blank line after the summary and before the footers |
| pre-commit-ban-terms | prevent the commit of certain tokens |
| pre-commit-lint | eslint js files in staging |
| pre-commit-package-lock | when a package.json is changed, check package-lock.json is versioned as well 
| pre-push-branch-pattern | insure a branch follows pattern `^(feat|fix|hotfix|chore|test|automation)/\d+-[\w-]+$` |
| pre-push-force-same-branch | should test that when you do a force push, your target the same remote branch and avoid the default one |
| pre-push-pull-rebase | pull the origin remote branch with the same name before push. Fetch default branch and warn if it's ahead |

## OPT Out

- If you encounter an issue with one of the hooks when you run the `push` command, you can temporarily
bypass the hooks with the option `--no-verify`.
- If you like having the warning but would prefer the commit or push not to fail, replace the line `exit $EXIT_CODE` in `pre-commit` and `pre-push` by `break` or just remove the line.
- You can disable the hooks any time by removing the files in `.git/hooks/`.

## Other idea of hook not implemented

- pre-push: when a tag is semver, check that it's defined in a package.json
- pre-commit: lint more languages
- pre-commit: check other languages Gemfile.lock, Pipfile.lock, Cargo.lock, composer.lock
- pre-push: check branch pattern for jira
- repare-commit-msg: extract kabanize or jira id from branch name and prepare a commit msg with a footer Issues: (#id)? < id >
- pre-push: prevent force push to a remote branch that has a different name

## Similar projects

- https://github.com/okonet/lint-staged
- https://github.com/folke/devmoji
- https://github.com/negokaz/git-fancy-message-prefix
- https://github.com/conventional-changelog/commitlint

## A word about hook managers

A note on hook managers. Below is a list. Each has advantages and flows.
Most of these managers are easy to use and easy to plug into.
Some like husky adds the feature of auto install of the hooks.
But in my opinion they all lack of good packages that are easy to search, download and customize. Therefore I find these package manager overcomplicated for the little they achieve.

- https://github.com/sds/overcommit
- https://github.com/evilmartians/lefthook
- https://github.com/pre-commit/pre-commit
- https://github.com/typicode/husky
