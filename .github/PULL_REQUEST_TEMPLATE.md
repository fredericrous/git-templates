<!--
The subject of the squashed commit must pass this repository's own commit-msg
hook: a conventional type prefix, a description of at most 50 characters after
it, and no emoji — the gitmoji is prepended for you.
-->

## What changed

<!-- One or two sentences. The diff shows the rest. -->

## Why

<!--
The part a reviewer cannot get from the diff: what goes wrong without this, and
what you considered instead and rejected. If this fixes a bug, say what shipped
broken and how it got past the tests that exist.
-->

## How it was verified

<!--
`make check` is the CI-parity target. If a behaviour changed, name the test that
would have failed before this. If a fix cannot be reproduced in a test, say so
and say why — sometimes that is legitimate, and sometimes it means the fix is
not understood yet.
-->

- [ ] `make check` passes locally (fmt, clippy `-D warnings`, the full suite)
- [ ] A behaviour change arrives with a test that fails without it
- [ ] Anything touching the commit path (`amont`, `amont-runtime`) adds no
      external crate — or makes the argument for reopening that rule, per
      `scripts/check-no-deps.sh`
- [ ] Docs under `docs/` updated, and any new page added to `docs/SUMMARY.md`

<!--
Closes #
-->
