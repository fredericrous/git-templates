//! Classifying an installed hook file against what it should be.
//!
//! The trap here is symmetrical and both halves are silent:
//!
//! - Compare an installed shim against the RAW tracked template and every one
//!   of the 96 repos reports as drifted, because installation deliberately
//!   substitutes the binary path. A tool that cries wolf on the whole fleet is
//!   a tool nobody reads.
//! - Compare too loosely — say, on length or on one marker line — and genuine
//!   drift is never seen, which is worse, because the report still looks calm.
//!
//! So classification is exact: recover the baked path from the installed file,
//! re-render the template with it, and require byte equality. Recovery IS the
//! comparison; there is no fuzzy middle.
//!
//! Note the placeholder appears three times in the template, one of them inside
//! a comment, because `make install` seds globally. Any comparison that assumed
//! a single substitution would misclassify every baked shim.

use std::path::Path;

use serde::Serialize;

/// The shim, embedded at build time so the tool reports correctly from any
/// directory rather than only inside a checkout. All four dispatcher templates
/// are one blob; `templates_are_still_one_blob` fails if that stops being true.
pub const TEMPLATE: &str = include_str!("../../../templates/hooks/pre-commit");

pub const PLACEHOLDER: &str = "__GITHOOKS_BIN__";

/// The four hook names git actually invokes, in the order the UI shows them.
pub const DISPATCHERS: [&str; 4] = ["commit-msg", "pre-commit", "pre-push", "prepare-commit-msg"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ShimState {
    /// Byte-identical to the template rendered with `baked`.
    Ok {
        baked: String,
    },
    /// Present, but not the template under any substitution.
    Drifted,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "bake", rename_all = "snake_case")]
pub enum BakeState {
    /// Points at the binary we would install.
    Current,
    /// Points somewhere else — the GUI-client failure mode, since a hook
    /// launched without an interactive PATH resolves nothing.
    Stale { path: String },
    /// The placeholder survived; resolution falls through to $HOME/.local/bin.
    Unbaked,
    /// The shims disagree with each other about where the binary is.
    Mixed,
    /// Nothing installed to have an opinion about.
    None,
}

pub fn render(binary: &str) -> String {
    TEMPLATE.replace(PLACEHOLDER, binary)
}

/// Recover the substituted path, or `None` if this file is not the template
/// under ANY substitution.
///
/// Solved from the template's literal segments rather than from a marker line.
/// An earlier version anchored on `BIN="…"` and silently recovered the WRONG
/// value: the template assigns `BIN="$GIT_HOOKS_BIN"` on an earlier line for
/// the escape hatch, so the first match was never the baked path. Every
/// correctly installed shim in the fleet would have reported as drifted.
///
/// Splitting on the placeholder cannot pick the wrong line, because the segments
/// around each occurrence are fixed text. The candidate is still only accepted
/// once re-rendering reproduces the file byte for byte, so a wrong guess is
/// rejected rather than believed.
pub fn recover_baked(installed: &str) -> Option<String> {
    let head = TEMPLATE.split(PLACEHOLDER).next()?;
    let rest = installed.strip_prefix(head)?;
    // The text that follows the first placeholder is fixed; whatever precedes
    // it is the substitution.
    let tail = TEMPLATE.split(PLACEHOLDER).nth(1)?;
    let end = if tail.is_empty() {
        rest.len()
    } else {
        rest.find(tail)?
    };
    let candidate = rest[..end].to_string();
    (render(&candidate) == installed).then_some(candidate)
}

pub fn classify(path: &Path) -> ShimState {
    let Ok(content) = std::fs::read_to_string(path) else {
        return ShimState::Missing;
    };
    match recover_baked(&content) {
        Some(baked) => ShimState::Ok { baked },
        None => ShimState::Drifted,
    }
}

/// Collapse the four shims' baked paths into one verdict.
pub fn bake_state(shims: &[ShimState], installed_binary: &str) -> BakeState {
    let mut baked: Vec<&str> = shims
        .iter()
        .filter_map(|s| match s {
            ShimState::Ok { baked } => Some(baked.as_str()),
            _ => None,
        })
        .collect();
    baked.sort_unstable();
    baked.dedup();

    match baked.as_slice() {
        [] => BakeState::None,
        [one] if *one == PLACEHOLDER => BakeState::Unbaked,
        [one] if *one == installed_binary => BakeState::Current,
        [one] => BakeState::Stale {
            path: (*one).to_string(),
        },
        _ => BakeState::Mixed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_are_still_one_blob() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../templates/hooks");
        for name in DISPATCHERS {
            let p = std::path::Path::new(dir).join(name);
            let got = std::fs::read_to_string(&p).expect("template");
            assert_eq!(
                got, TEMPLATE,
                "{name} differs from the embedded template; embedding one blob \
                 is no longer valid and classification would be wrong for it"
            );
        }
    }

    /// The placeholder appears three times, one inside a comment. A renderer
    /// that substituted only the executable occurrences would produce a file
    /// that never matches anything `make install` writes.
    #[test]
    fn every_occurrence_is_substituted() {
        assert_eq!(TEMPLATE.matches(PLACEHOLDER).count(), 3);
        let out = render("/opt/githooks");
        assert!(!out.contains(PLACEHOLDER));
        assert_eq!(out.matches("/opt/githooks").count(), 3);
    }

    #[test]
    fn a_correctly_baked_shim_is_not_drift() {
        let installed = render("/Users/me/.local/bin/githooks");
        assert_eq!(
            recover_baked(&installed).as_deref(),
            Some("/Users/me/.local/bin/githooks"),
            "the whole fleet would read as drifted"
        );
    }

    #[test]
    fn an_unbaked_shim_recovers_the_placeholder() {
        assert_eq!(recover_baked(TEMPLATE).as_deref(), Some(PLACEHOLDER));
    }

    /// The other half of the trap: real drift must not be waved through.
    #[test]
    fn genuine_drift_is_detected() {
        let mut edited = render("/opt/githooks");
        edited.push_str("\n# someone added this\n");
        assert_eq!(recover_baked(&edited), None);

        let hand_written = "#!/bin/sh\nBIN=\"/opt/githooks\"\nexec \"$BIN\" \"$@\"\n";
        assert_eq!(
            recover_baked(hand_written),
            None,
            "a plausible-looking file that is not our template must not pass"
        );
    }

    /// A candidate that reproduces the file is required — finding `BIN="…"` is
    /// not enough on its own.
    #[test]
    fn the_anchor_alone_does_not_satisfy_it() {
        let faked = render("/opt/a").replace("exec", "# exec");
        assert!(faked.contains("BIN=\"/opt/a\""));
        assert_eq!(recover_baked(&faked), None);
    }

    fn ok(p: &str) -> ShimState {
        ShimState::Ok { baked: p.into() }
    }

    #[test]
    fn bake_states() {
        assert_eq!(bake_state(&[ok("/bin/gh")], "/bin/gh"), BakeState::Current);
        assert_eq!(
            bake_state(&[ok("/old/gh")], "/bin/gh"),
            BakeState::Stale {
                path: "/old/gh".into()
            }
        );
        assert_eq!(
            bake_state(&[ok(PLACEHOLDER)], "/bin/gh"),
            BakeState::Unbaked
        );
        assert_eq!(
            bake_state(&[ok("/a"), ok("/b")], "/bin/gh"),
            BakeState::Mixed
        );
        assert_eq!(
            bake_state(&[ShimState::Missing], "/bin/gh"),
            BakeState::None
        );
        // Four identical shims are Current, not Mixed.
        assert_eq!(
            bake_state(
                &[ok("/bin/gh"), ok("/bin/gh"), ok("/bin/gh"), ok("/bin/gh")],
                "/bin/gh"
            ),
            BakeState::Current
        );
    }
}
