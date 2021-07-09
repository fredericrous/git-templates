# Git Templates with hooks

Git Starter Template with Simple hooks for your development workflow.

Here to inspire you create the git hooks that fit you

<img src="https://user-images.githubusercontent.com/702227/125003867-1b012f00-e050-11eb-8641-748ef806c639.png" width="800">

*If there is an issue with a hook, please open an issue and consult the section [OPT Out](#opt-out).*

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

- https://git-scm.com/docs/git-commit
- https://www.conventionalcommits.org/en/v1.0.0/
- https://gitmoji.dev/

Once the message complies with the checks, the commit is finally created.

### Pre-push

When you run the command `git push`, the hook `pre-push` gets triggered. All the scripts prefixed with `pre-push` get executed in sequence. See section [Hooks implemented](#hooks-implemented).

<img src="https://user-images.githubusercontent.com/702227/125002910-ea1ffa80-e04d-11eb-8d71-dcb8339d45a1.png" width="800">

Some of the hooks give you recommendation to help you resolve the branch state you are in

<img src="https://user-images.githubusercontent.com/702227/125002967-0328ab80-e04e-11eb-9fca-5944a2f42eb5.png" width="800">

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
|-|-|
| commit-msg | Lint commit message to conform to [git recommentation](https://git-scm.com/docs/git-commit) and mostly to[conventional commits]( https://www.conventionalcommits.org/en/v1.0.0/) specification<br>Prefix with an emoji conform to [gitmoji](https://gitmoji.dev/) specification<br>Apply formating to the commit: break the body to 72 char per line. It groups footers. It insures the body is separated by a blank line after the summary and before the footers |
| pre-commit-ban-terms | prevent the commit of certain tokens |
| pre-commit-lint | eslint js files in staging |
| pre-commit-package-lock | when a package.json is changed, check package-lock.json is versioned as well 
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
But in my opinion they don't provide enough different packages. They don't provide an easy way to search and download this packages.
Also I find these package manager overcomplicated for the little feature that git hooks is, there is a learning curve for each of these managers.
In comparaison, the hooks on this repository are loaded by a simple for loop in a zsh script. It's simple, effective and easy to customize.

- https://github.com/sds/overcommit [Ruby]
- https://github.com/evilmartians/lefthook [Go]
- https://github.com/pre-commit/pre-commit [Python]
- https://github.com/typicode/husky [Javascript]
