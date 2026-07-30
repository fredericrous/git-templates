# Interactive `hook.skip` management

Status: **specification**. Nothing here is built.

Extends `docs/fleet-dashboard.md`, which listed the `s` toggle as a v2 item
without saying what it should do.

## The problem is not ergonomics

The obvious framing is that turning a check off is awkward — you must know both
the config key and the exact check name. That is true and it is the least
interesting part.

The real problem is that **`hook.skip` matches by substring**, and the
dispatcher applies it as `check_name.contains(skip_value)`:

```rust
list.iter().filter(|n| !skips.iter().any(|s| n.contains(s.as_str())))
```

Measured against the 20 checks that exist today:

| `hook.skip` value | suppresses |
|---|---|
| `pre-commit-clippy` | 1 / 20 |
| `clippy` | 1 / 20 |
| `cargo` | 2 / 20 |
| `test` | 2 / 20 |
| `lint` | 5 / 20 |
| `pre-commit` | **15 / 20** |
| `t` | **19 / 20** |
| `pre` | **20 / 20** |
| `e` | **20 / 20** |

None of those are adversarial. `t` is a plausible shorthand for "tests"; `pre`
is a plausible shorthand for "prettier". Either one silently disables the entire
check suite, in a config file nobody reads, with no output at commit time saying
so. A developer would discover it when something reached CI that a hook exists
to catch.

So the feature is not "let me type less". It is **make the blast radius visible
before the write happens**, and make an existing over-broad skip discoverable
after it. Everything below follows from that.

## Non-goals

- **Not a replacement for `--no-verify`.** A one-off bypass is git's job.
  `hook.skip` is for a persistent decision, and the UI should not blur them.
- **Not a scheduler.** No expiry dates, no "skip until Friday". A skip that
  expires silently is a different surprise, not a smaller one.
- **Not fleet-wide bulk writes.** See "Scope", below — for a fleet-wide skip the
  correct mechanism is one global config entry, not 96 local ones.

## Rules

**1. The UI writes the full check name, never a fragment.**
`pre-commit-clippy`, not `clippy`. Both suppress exactly one check today, but
the fragment is a bet on every future check name — adding
`pre-commit-clippy-nursery` would silently widen an existing `clippy` skip. The
minimal blast radius is the only defensible default, and the UI is the one
writer that can guarantee it.

**2. Every write shows what it suppresses, by name, before it happens.**
Consistent with the dashboard's diff-first rule for destructive actions. Turning
off a check that catches committed secrets deserves the same confirmation as
deleting a file.

```
 skip pre-commit-ban-terms in Perso/homelab?

   git config --add hook.skip pre-commit-ban-terms

   suppresses 1 of 20 checks:
     pre-commit-ban-terms

   [y] write   [esc] cancel
```

**3. An over-broad EXISTING skip is reported as a finding.**
The UI cannot stop someone writing `hook.skip = e` by hand, so it must surface
it. A skip suppressing more than one check appears in the repo detail view with
its full expansion, and in the hook view as the reason a check shows `skipped`:

```
 hook.skip
   pre-commit-clippy    -> pre-commit-clippy
   t                    -> 19 checks  !  probably not intended
```

The threshold is "more than one", not a percentage. Two is already a surprise if
you meant one.

**4. Removal is exact, and the exit code is not evidence.**

`git config --unset hook.skip <value>` takes a value-pattern that is a **regex**,
not a literal. Measured:

- `--unset hook.skip 'pre-commit-lint.js'` removes `pre-commit-lint-js` — the
  `.` is a wildcard. Any value the UI does not escape can over-match.
- When the pattern matches **more than one** value, git prints
  `warning: hook.skip has multiple values`, **removes nothing, and exits 0**.
  So `--unset hook.skip clippy` against a config holding both `clippy` and
  `pre-commit-clippy` is a silent no-op that reports success.
- `--unset-all` with the same fragment removes **both**, which is the opposite
  surprise.

So the UI must pass an anchored, escaped pattern (`^pre-commit-clippy$`) and
then **re-read the config to confirm the value is gone**. The exit code cannot
distinguish "removed it" from "declined to act". This is the same posture the
apply path already takes — report what happened, never what was intended — and
here there is a measured reason for it rather than a principle.

**5. Idempotent.**
`--add` on a value already present creates a duplicate that must then be unset
twice. The UI checks first and reports "already skipped" rather than writing.

## Scope: local, or global

Two scopes, and the distinction matters more than it looks.

- **Local** (`git config --add`, writes `.git/config`) — this repo only. The
  default, because it is the reversible one and it is where a
  repo-specific decision belongs.
- **Global** (`git config --global --add`) — every repo on the machine,
  including ones cloned later.

A fleet-wide skip must use **global**, not a loop over 96 local writes. One
entry is one thing to find and one thing to undo; 96 entries are a migration in
their own right, and the tool has already learned what a half-applied sweep
costs. The UI therefore offers global explicitly rather than synthesising it,
and labels it as affecting every repository — present and future.

`configured_skips` reads via plain `git config --get-all`, so global and local
entries are already merged with no way to tell them apart at dispatch time. The
detail view must show which scope each value came from
(`git config --show-origin --get-all hook.skip`), or a developer will delete a
repo's `.git/config` line and be baffled that the check is still skipped.

## Where it lives in the UI

**Repo detail (`Enter`, then `s`)** — the check list becomes selectable, and `s`
toggles the highlighted check for that repo. This is the common case: "not in
this repo, not right now".

**Hook view (`h`, then `s`)** — offers the *global* skip for the selected check,
with the affected-repo count from the `APPLICABLE` column. This is the rarer,
larger decision, and it should feel larger.

Both go through the same preview. Neither writes on a single keypress.

## Data model

```
SkipEntry {
  value      : String       // as written in config
  scope      : Local | Global | Other(path)
  suppresses : Vec<&'static str>   // resolved against the check registry
}

SkipPlan {
  repo       : PathBuf
  scope      : Scope
  action     : Add(String) | Remove(String)
  command    : Vec<String>          // exactly what will be run
  suppresses : Vec<&'static str>    // what changes as a result
  refuse     : Option<Refusal>      // already present / not applicable / unreadable config
}
```

`suppresses` is resolved through the same substring rule the dispatcher uses.
Reimplementing the match differently is how a UI comes to claim a check is
active while the dispatcher skips it — the existing `checks::is_skipped` already
shares the rule and this must too.

## Tests

The important ones are about honesty, not mechanics:

- A one-character skip is reported as suppressing 19–20 checks, by name.
- The written value is always the full check name — falsified by writing a
  fragment and asserting the test fails.
- Adding an existing skip is refused, not duplicated.
- Removal uses an anchored pattern and is verified by re-reading config.
- A global skip is labelled as global in the detail view, with its origin.
- The resolver agrees with `dispatch::selected` for the same inputs — a property
  test over the check list is cheap here and pins the two together.

## Open questions

1. **Should the UI ever refuse a write?** Showing that `e` suppresses everything
   may be enough. Refusing outright protects against a mis-keystroke but invites
   working around the tool by hand, which is worse.
2. **Should `skips` in the fleet table show a count or the values?** A count is
   compact and hides an over-broad entry; the values do not fit in a column.
   Probably a count plus a marker when any entry suppresses more than one.
3. **Is `--show-origin` fast enough per repo at fleet scale?** It is one extra
   `git config` invocation across ~96 repos. Measure before adding it to the
   scan rather than the detail view.
