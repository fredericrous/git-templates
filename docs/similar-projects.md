# Similar projects, and what was taken from them

Migrated from the old wiki's "Similar projects" page. The list of projects is
unchanged. The paragraph that followed it is not, because it argued a position
this project no longer holds — see below.

## Hook managers

- [pre-commit](https://github.com/pre-commit/pre-commit) [Python]
- [lefthook](https://github.com/evilmartians/lefthook) [Go]
- [husky](https://github.com/typicode/husky) [JavaScript]
- [overcommit](https://github.com/sds/overcommit) [Ruby]

## Individual hooks and adjacent tools

- [lint-staged](https://github.com/okonet/lint-staged)
- [commitlint](https://github.com/conventional-changelog/commitlint)
- [devmoji](https://github.com/folke/devmoji)
- [git-fancy-message-prefix](https://github.com/negokaz/git-fancy-message-prefix)
- [greg0ire/git_template](https://github.com/greg0ire/git_template)

## What the wiki used to say, and why it changed

The wiki's position was:

> The approach here is not to try to create a new challenger. But to suggest a
> different approach, keep it simple with a template that is easy to modify.
> You don't have zsh? convert the script to bash or powershell. You don't have
> nodejs? don't use the `*.js` hooks, or find others that fit you.

That was an honest description of a repository of shell scripts you were meant
to fork and edit. It is not a description of this one. "Edit the script to suit
you" means your changes live in a file the next update overwrites, invisible to
anyone else on the team — and it is exactly the answer the old
[opt-out page](opting-out.md) gave, which is why that page now gives a
different one.

The parts of the position that survived: a check you turn off should be turned
off by *configuration you can read back*, not by a fork; and nothing should
require a runtime you did not already have. The zsh and NodeJS requirements
that prompted "convert the script to bash" are gone — it is one binary now.

## What was actually taken from the others

Recorded properly in
[index fidelity and run modes](index-fidelity-and-run-modes.md), which reviews
`pre-commit`, `lefthook` and `husky` as designs rather than as competitors, and
says what was refused as well as what was adopted. In short:

- **From `pre-commit`:** a repository declaring its own checks in a committed
  file, so a team shares a check by committing it rather than by each member
  installing it by hand. Taken, as
  [`.githooks.conf`](custom-checks.md) — but with a
  [trust gate](trust.md) in front of it, because a committed manifest is a
  committed *command*, and cloning a repository is not consent to run it.
- **Refused:** managing tool installation. `pre-commit` bootstraps
  language-specific environments; here, a check that cannot find its tool
  either warns and skips (Kubernetes, JSON/YAML) or fails loudly, and the tool
  stays your problem. That keeps the commit path from becoming a package
  manager.
- **From all three:** the observation that filename-prefixed
  `.git/hooks/pre-commit-*` scripts can never be shared, because
  `.git/hooks` is not committed.
