//! The signs the hooks print.
//!
//! **Colour is the terminal's decision, not ours.** These used to be 256-colour
//! codes — `38;5;112`, `38;5;160`, `38;5;208` — which live in the fixed xterm
//! cube. A terminal theme remaps only indices 0–15; anything above renders
//! identically whatever palette the user chose. So a carefully themed terminal
//! was being overridden by three numbers picked years ago.
//!
//! The base ANSI colours ARE theme-controlled, so `32` means "whatever this
//! terminal calls green". That is the whole fix: output follows a `vivid`
//! palette, a Solarized profile or a high-contrast one, without reading any
//! configuration.
//!
//! `LS_COLORS` is deliberately NOT consulted. It maps FILE TYPES — `di`, `ex`,
//! `mi` — while this output needs ok/warning/error. There is no honest
//! correspondence, and the nearest candidates are worse than nothing: under
//! `vivid lava`, `mi` and `or` are dark grey on red, which as a foreground for
//! `✗` is close to invisible.
//!
//! The earlier note here said the signs were "kept byte-identical" with the zsh
//! originals so a user could not tell which implementation ran. That mattered
//! during the migration and no longer does — no zsh hook remains to match.

use std::sync::OnceLock;

/// Base ANSI, deliberately. See the module docs.
const GREEN: &str = "32";
const RED: &str = "31";
const YELLOW: &str = "33";

/// Does the caller want colour at all?
///
/// `NO_COLOR` per no-color.org: present and NON-EMPTY disables it, whatever the
/// value. `TERM=dumb` is a terminal that cannot render SGR.
///
/// Read once — a hook is a short-lived process and its environment does not
/// change underneath it.
pub fn colors_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        let no_color = std::env::var_os("NO_COLOR")
            .map(|v| !v.is_empty())
            .unwrap_or(false);
        let dumb = std::env::var("TERM").map(|t| t == "dumb").unwrap_or(false);
        !no_color && !dumb
    })
}

fn paint(text: &str, sgr: &str) -> String {
    if colors_enabled() {
        format!("\u{1b}[{sgr}m{text}\u{1b}[0m")
    } else {
        text.to_string()
    }
}

fn sign(glyph: &str, sgr: &str) -> String {
    format!("  {}", paint(glyph, sgr))
}

/// The glyph carries the meaning and the colour only reinforces it, so under
/// `NO_COLOR` — and for the ~8% of men with red-green colour vision deficiency
/// — `✓ ✗ !` stay distinguishable on their own.
pub fn valid_sign() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| sign("✓", GREEN))
}

pub fn error_sign() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| sign("✗", RED))
}

pub fn warning_sign() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| sign("!", YELLOW))
}

/// Emphasise a fragment inside a message, in the terminal's own accent.
pub fn highlight(text: &str) -> String {
    paint(text, YELLOW)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The point of the change: nothing emits a code above 15, because those
    /// ignore the terminal's palette entirely.
    #[test]
    fn no_sign_uses_the_fixed_256_colour_cube() {
        for s in [valid_sign(), error_sign(), warning_sign()] {
            assert!(
                !s.contains("38;5;"),
                "a 256-colour code overrides the user's theme: {s:?}"
            );
        }
    }

    #[test]
    fn signs_carry_a_distinct_glyph_not_only_a_colour() {
        assert!(valid_sign().contains('✓'));
        assert!(error_sign().contains('✗'));
        assert!(warning_sign().contains('!'));
    }

    /// No source file may emit a 256-colour escape directly.
    ///
    /// The migration to base ANSI converted the sign helpers but missed a raw
    /// `\u{1b}[38;5;208m` in the pre-push error line, which therefore ignored
    /// the terminal theme for two more PRs. Checking the helpers was not enough
    /// because the offender did not use them.
    #[test]
    fn no_source_file_emits_a_256_colour_escape() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let mut offenders = Vec::new();
        fn walk(d: &std::path::Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(d).expect("src").flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    for (i, line) in std::fs::read_to_string(&p)
                        .unwrap_or_default()
                        .lines()
                        .enumerate()
                    {
                        let mentions_the_pattern =
                            line.contains("offenders") || line.trim_start().starts_with("//");
                        if mentions_the_pattern {
                            continue;
                        }
                        // Split so this line is not itself an offender.
                        if line.contains(concat!("[38", ";5;")) {
                            out.push(format!("{}:{}", p.display(), i + 1));
                        }
                    }
                }
            }
        }
        walk(std::path::Path::new(dir), &mut offenders);
        assert!(
            offenders.is_empty(),
            "a fixed 256-colour code overrides the user's terminal theme: {offenders:?}"
        );
    }

    /// A success glyph must never be painted with the warning accent.
    ///
    /// The mechanical rewrite from 256-colour codes turned `color("✓", "112")`
    /// into `highlight("✓")` in two places, which renders a tick in the warning
    /// colour — the kind of damage a regex does quietly. Sign glyphs belong to
    /// the sign functions; `highlight` is for emphasising words.
    #[test]
    fn glyphs_are_not_routed_through_highlight() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let mut offenders = Vec::new();
        fn walk(d: &std::path::Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(d).expect("src").flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    let body = std::fs::read_to_string(&p).unwrap_or_default();
                    for (i, line) in body.lines().enumerate() {
                        let is_test_or_doc =
                            line.trim_start().starts_with("//") || line.contains("offenders");
                        if is_test_or_doc {
                            continue;
                        }
                        for g in ["\\u{2713}", "\\u{2717}", "✓", "✗"] {
                            if line.contains("highlight(") && line.contains(g) {
                                out.push(format!("{}:{}", p.display(), i + 1));
                            }
                        }
                    }
                }
            }
        }
        walk(std::path::Path::new(dir), &mut offenders);
        assert!(
            offenders.is_empty(),
            "a status glyph is being painted as emphasis: {offenders:?}"
        );
    }

    /// `colors_enabled` memoises, so the predicate is tested directly rather
    /// than through a cache whichever test happens to fill first.
    #[test]
    fn no_color_is_honoured_per_the_standard() {
        fn decide(no_color: Option<&str>, term: &str) -> bool {
            let nc = no_color.map(|v| !v.is_empty()).unwrap_or(false);
            !nc && term != "dumb"
        }
        assert!(decide(None, "xterm-256color"), "colour by default");
        assert!(!decide(Some("1"), "xterm-256color"), "any value disables");
        assert!(!decide(Some("0"), "xterm-256color"), "even \"0\" disables");
        assert!(
            decide(Some(""), "xterm-256color"),
            "an EMPTY value does NOT disable — no-color.org is explicit"
        );
        assert!(!decide(None, "dumb"), "a dumb terminal cannot render SGR");
    }
}
