# Git Templates with hooks

Git Starter Template with Simple hooks for your development workflow.

Here to inspire you create the git hooks that fit you

<img src="https://user-images.githubusercontent.com/702227/125003867-1b012f00-e050-11eb-8641-748ef806c639.png" width="800">

*If there is an issue with a hook, please open an issue and consult the section [Opt Out](#opt-out).*

Want to start right away? Check the [Requirements](#requirements) and head to the section [Setup](#setup).

## The workflow

You are working on a badass feature. Now's the time to share your work.
You hit `git commit`, a nice template `message` appears to help you write a meaningful log description.
Or maybe you are lazy and just write `git commit -m"Add to Cart"`.

### Pre-commit

Git triggers the `pre-commit` hook before the commit actually occurs. The hook calls in parallel all the scripts prefixed with `pre-commit-`. These scripts check the staged files for code that shouldn't be commited as is and more, see section [Hooks implemented](#hooks-implemented).

<img src="https://user-images.githubusercontent.com/702227/125003011-1f2c4d00-e04e-11eb-83d0-219c903fb475.png" width="800">

If one check fail, the commit is aborted.

<img src="https://user-images.githubusercontent.com/702227/125003095-5d297100-e04e-11eb-85cf-967dfc9c34da.png" width="800">

Once the tests have passed, the next hook is triggered, `commit-msg`.

### Commit-msg

This hook lints your message, to help to keep some conventions in place, and it happens an emoji to your commit.

<img src="https://user-images.githubusercontent.com/702227/125004472-7d0e6400-e051-11eb-87b3-a48391c937ee.png" width="800">

Here are the conventions the hooks from this repository takes inspiration from:

- <https://git-scm.com/docs/git-commit>
- <https://www.conventionalcommits.org/en/v1.0.0/>
- <https://gitmoji.dev/>

Once the message complies with the checks, the commit is finally created.

### Pre-push

When you run the command `git push`, the hook `pre-push` gets triggered. All the scripts prefixed with `pre-push` get executed in sequence. See section [Hooks implemented](#hooks-implemented).

<img src="https://user-images.githubusercontent.com/702227/125002910-ea1ffa80-e04d-11eb-8d71-dcb8339d45a1.png" width="800">

Some of the hooks give you recommendation to help you resolve the branch state you are in

<img src="https://user-images.githubusercontent.com/702227/125002967-0328ab80-e04e-11eb-9fca-5944a2f42eb5.png" width="800">

## Requirements

- Git 2.22+
- ZSH
- NodeJS (minimum v6)
- [ripgrep](https://github.com/BurntSushi/ripgrep/)

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

## Hooks implemented

| hook | description |
|-|-|
| commit-msg | Lint commit message to conform to [git recommentation](https://git-scm.com/docs/git-commit) and mostly to[conventional commits]( https://www.conventionalcommits.org/en/v1.0.0/) specification<br>Prefix with an emoji conform to [gitmoji](https://gitmoji.dev/) specification<br>Apply formating to the commit: break the body to 72 char per line. It groups footers. It insures the body is separated by a blank line after the summary and before the footers |
| prepare-commit-msg | Extract Jira or Kanbanize id from branch name if there is, and append it to the footer of the commit msg |
| pre-commit-ban-terms | prevent the commit of certain tokens |
| pre-commit-lint-js | eslint js files in staging |
| pre-commit-lint-json-yaml | lint json and yaml with [yq](https://github.com/mikefarah/yq) |
| pre-commit-merge-conflict | detect for staged files in a merge states |
| pre-commit-package-lock | when a package.json is changed, check package-lock.json is versioned as well
| pre-commit-usual-name | Issue a warning when you use a commit user for the first time |
| pre-push-branch-pattern | insure a branch follows pattern `prefix/digit-branch-name`. ie `feat/3002-image-crop` |
| pre-push-force-same-branch | should test that when you do a force push, your target the same remote branch and avoid the default one |
| pre-push-pull-rebase | pull the origin remote branch with the same name before push. Fetch default branch and warn if it's ahead |

## Commit Prefix

```sh
# Uncomment one of the following templates
#👷   build:
# └ Add or update CI build system
#🔧   chore:
# └ Changes to the configuration files or auxiliary tools and libraries
#    such as documentation generation
#🔧   chore(deps):
# └ Upgrade or Downgrade a dependency
#🔧   chore(security):
# └ Fix security issues
#📝   docs:
# └ Documentation only changes
#✨   feat:
# └ Introduce new features
#🐛   fix:
# └ Fix a bug
#⚡️   perf:
# └ Improves performance
#♻️  refactor:
# └ A code change that neither fixes a bug nor adds a feature
#⏪️   revert:
# └ Revert changes. Ideally done with the `git revert` command
#🎨   style:
# └ Improve structure / format of the code
#🚨   test(tdd):
# └ Add a failing test
#🚨   test:
# └ Add, update, or pass tests
#➕    add:
# └ Add files as part of a larger feature
#➖    remove:
# └ opposite of add:
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

## Opt Out

**Bypass one `pre-push` or `pre-commit` hook**

You can use the flag `-c hook.skip=<name of hook>` to disable temporarely one hook. You can cumulate the `-c` flags to skip more hooks.

`<name of hook>` works as a glob pattern. For example `-c hook.skip=lint` would match `pre-commit-lint-js` and `pre-commit-lint-json-yaml`.

You can set the flag permanently for the repository with `git config --add hook.skip <name of hook>`

**Bypass all hooks on `pre-push` or `pre-commit`**

Append the option `--no-verify` to your `git push` or `git commit` command

Note: the prepare-commit-msg and commit-msg hooks can't be bypassed this way

**Warn instead of fail the commit**

If you like having the warning but would prefer the commit or push not to fail, replace the line `exit $EXIT_CODE` in `pre-commit` and `pre-push` by `break` or just remove the line.

**Uninstall**

You can disable the hooks any time by removing the files in `$(git rev-parse --git-dir)/hooks/`.

## Other ideas of hook not implemented

- post-commit: tag automatically when package version has been incremented (might be armful depending on your CD workflow)
- pre-commit: lint more languages
- pre-commit: check other languages Gemfile.lock, Pipfile.lock, Cargo.lock, composer.lock
- pre-commit: check you are commiting with the usual gpg key (could be slow)
- pre-push: check branch pattern for jira id
- pre-push: prevent force push to a remote branch that has a different name (amolst impossible?)
- commit-msg: commit msg alias. exemple: "."="(prev prefix): more on $(previous commit msg)"
- commit-msg: force user to put a description that contains more than 3 words and a body that contains more than 5 words

Note: Functionallities that are covered by gitattributes and gitignore shouldn't be implemented as hooks.

## Similar projects

- <https://github.com/okonet/lint-staged>
- <https://github.com/folke/devmoji>
- <https://github.com/negokaz/git-fancy-message-prefix>
- <https://github.com/conventional-changelog/commitlint>
- <https://github.com/greg0ire/git_template>

## A word about hook managers

A note on hook managers. Below is a list. Each has advantages and flows. Some have prebuilt hooks that the others don't.
The approach here is not to try to create a new challenger.
But to suggest a different approach, keep it simple with a template that is easy to modify.
You don't have zsh? convert the script to bash or powershell. You don't have nodejs? don't use the *.js hooks, or find others that fit you.

- <https://github.com/sds/overcommit> [Ruby]
- <https://github.com/evilmartians/lefthook> [Go]
- <https://github.com/pre-commit/pre-commit> [Python]
- <https://github.com/typicode/husky> [Javascript]
