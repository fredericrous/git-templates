//! The dashboard.
//!
//! Rendering is a pure function of state, which is the only reason the spec's
//! success criterion can be a test rather than an intention: `TestBackend`
//! draws into a buffer and the assertion reads it back. A criterion nobody can
//! run is a wish.
//!
//! Layout follows Shneiderman's mantra — a fleet summary, then a filterable
//! table, then detail on demand. Every count carries its denominator, because
//! the two failures that motivated this tool were both a bare scalar printed
//! with equal confidence whether it had measured everything or nothing.
//!
//! State is encoded twice, glyph AND colour, and never by colour alone: roughly
//! 8% of men have red-green colour vision deficiency, and `NO_COLOR` must leave
//! a fully usable screen.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};

use crate::scan::{FleetScan, Repo};
use crate::shim::{BakeState, ShimState, DISPATCHERS};

pub struct App {
    pub scan: FleetScan,
    pub selected: usize,
    pub detail: bool,
    pub filter: String,
    pub filtering: bool,
    pub scanning: bool,
    pub visited: usize,
    pub elapsed: f64,
    pub quit: bool,
}

impl App {
    pub fn new(scan: FleetScan) -> Self {
        App {
            scan,
            selected: 0,
            detail: false,
            filter: String::new(),
            filtering: false,
            scanning: false,
            visited: 0,
            elapsed: 0.0,
            quit: false,
        }
    }

    pub fn rows(&self) -> Vec<&Repo> {
        self.scan
            .repos
            .iter()
            .filter(|r| {
                self.filter.is_empty()
                    || r.path
                        .to_string_lossy()
                        .to_lowercase()
                        .contains(&self.filter.to_lowercase())
            })
            .collect()
    }

    pub fn on_key(&mut self, key: char, is_enter: bool, is_esc: bool, is_backspace: bool) {
        if self.filtering {
            if is_esc {
                self.filtering = false;
                self.filter.clear();
            } else if is_enter {
                self.filtering = false;
            } else if is_backspace {
                self.filter.pop();
            } else if key != '\0' {
                self.filter.push(key);
            }
            self.selected = 0;
            return;
        }
        let len = self.rows().len();
        match (key, is_enter, is_esc) {
            ('q', _, _) => self.quit = true,
            ('/', _, _) => self.filtering = true,
            ('j', _, _) if len > 0 => self.selected = (self.selected + 1).min(len - 1),
            ('k', _, _) => self.selected = self.selected.saturating_sub(1),
            (_, true, _) if len > 0 => self.detail = true,
            (_, _, true) => {
                if self.detail {
                    self.detail = false
                } else {
                    self.filter.clear()
                }
            }
            _ => {}
        }
    }
}

/// `●` ok, `◐` drifted, `○` missing — position encodes WHICH hook, so the
/// column costs four characters instead of four names.
fn shim_glyphs(r: &Repo) -> String {
    r.shims
        .iter()
        .map(|s| match s {
            ShimState::Ok { .. } => '●',
            ShimState::Drifted => '◐',
            ShimState::Missing => '○',
        })
        .collect()
}

fn bake_word(b: &BakeState) -> &'static str {
    match b {
        BakeState::Current => "current",
        BakeState::Stale { .. } => "stale",
        BakeState::Unbaked => "unbaked",
        BakeState::Mixed => "mixed",
        BakeState::None => "-",
    }
}

/// A redundant text summary of the same information the glyphs carry, so the
/// screen survives NO_COLOR and colour vision deficiency.
fn state_word(r: &Repo) -> String {
    if !r.managed {
        return "! unmanaged".into();
    }
    let missing = r
        .shims
        .iter()
        .filter(|s| matches!(s, ShimState::Missing))
        .count();
    let drifted = r
        .shims
        .iter()
        .filter(|s| matches!(s, ShimState::Drifted))
        .count();
    if drifted > 0 {
        format!("x drifted {drifted}")
    } else if missing > 0 {
        format!("x missing {missing}")
    } else if !r.stale_ours.is_empty() || !r.foreign_subs.is_empty() || r.hook_pkgjson {
        "! leftovers".into()
    } else if matches!(r.baked, BakeState::Stale { .. } | BakeState::Mixed) {
        "! stale bake".into()
    } else {
        "ok".into()
    }
}

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let chunks = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(area);

    header(f, chunks[0], app);
    if app.detail {
        detail(f, chunks[1], app);
    } else if app.scan.looks_like_a_failed_scan() && !app.scanning {
        failure(f, chunks[1], app);
    } else {
        table(f, chunks[1], app);
    }
    footer(f, chunks[2], app);
}

fn header(f: &mut Frame, area: Rect, app: &App) {
    let s = &app.scan;
    let status = if app.scanning {
        format!(
            "scanning · {} directories · {:.1}s",
            app.visited, app.elapsed
        )
    } else {
        format!("{} directories · {:.1}s", s.dirs_visited, app.elapsed)
    };
    let text = vec![
        Line::from(format!("{}   {status}", s.root.display())),
        Line::from(format!(
            "{} repositories · {} managed · {} unmanaged · {} skipped subtrees",
            s.git_dirs_found, s.managed_seen, s.unmanaged_seen, s.excluded_dirs
        )),
        Line::from(format!(
            "consistency  {}",
            DISPATCHERS
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    let ok = s
                        .repos
                        .iter()
                        .filter(|r| r.managed)
                        .filter(|r| matches!(r.shims.get(i), Some(ShimState::Ok { .. })))
                        .count();
                    // N/M, never a bare adjective: the number that proves fleet
                    // health is the one the old text sweep got wrong.
                    format!("{n} {ok}/{}", s.managed_seen)
                })
                .collect::<Vec<_>>()
                .join("  ")
        )),
    ];
    f.render_widget(
        Paragraph::new(text).block(Block::default().borders(Borders::BOTTOM)),
        area,
    );
}

/// The screen this tool exists for. An empty table must never read as a calm,
/// clean fleet.
fn failure(f: &mut Frame, area: Rect, app: &App) {
    let s = &app.scan;
    let mut lines = vec![
        Line::from(format!("No repositories found under {}", s.root.display())),
        Line::from(""),
        Line::from(format!(
            "Visited {} directories in {:.1}s and found 0 git repositories.",
            s.dirs_visited, app.elapsed
        )),
        Line::from("This is a SCAN FAILURE, not a clean fleet."),
        Line::from(format!(
            "  - is --root correct?      (currently: {})",
            s.root.display()
        )),
        Line::from(format!(
            "  - is --depth deep enough? (currently: {})",
            s.depth
        )),
    ];
    if !s.unreadable.is_empty() {
        lines.push(Line::from(format!(
            "  - {} path(s) could not be read",
            s.unreadable.len()
        )));
    }
    f.render_widget(Paragraph::new(lines), area);
}

fn table(f: &mut Frame, area: Rect, app: &App) {
    let rows = app.rows();
    // Narrow terminals drop columns rather than scroll sideways.
    let wide = area.width >= 100;
    let mid = area.width >= 76;

    let header = if wide {
        vec!["REPO", "SHIMS", "BAKE", "LANG", "SKIPS", "STATE"]
    } else if mid {
        vec!["REPO", "SHIMS", "BAKE", "STATE"]
    } else {
        vec!["REPO", "STATE"]
    };

    let body: Vec<Row> = rows
        .iter()
        .map(|r| {
            let path = r.path.to_string_lossy().into_owned();
            let mut cells = vec![Cell::from(path)];
            if mid {
                cells.push(Cell::from(shim_glyphs(r)));
                cells.push(Cell::from(bake_word(&r.baked)));
            }
            if wide {
                cells.push(Cell::from(r.languages.join(" ")));
                cells.push(Cell::from(if r.skips.is_empty() {
                    "-".to_string()
                } else {
                    r.skips.len().to_string()
                }));
            }
            cells.push(Cell::from(state_word(r)));
            Row::new(cells)
        })
        .collect();

    let widths: Vec<Constraint> = if wide {
        vec![
            Constraint::Min(28),
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Length(12),
            Constraint::Length(6),
            Constraint::Length(14),
        ]
    } else if mid {
        vec![
            Constraint::Min(20),
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Length(14),
        ]
    } else {
        vec![Constraint::Min(12), Constraint::Length(14)]
    };

    let mut state = TableState::default();
    state.select(Some(app.selected.min(rows.len().saturating_sub(1))));
    f.render_stateful_widget(
        Table::new(body, widths)
            .header(Row::new(header))
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> "),
        area,
        &mut state,
    );
}

fn detail(f: &mut Frame, area: Rect, app: &App) {
    let rows = app.rows();
    let Some(r) = rows.get(app.selected) else {
        return;
    };
    let mut lines = vec![
        Line::from(r.path.to_string_lossy().into_owned()),
        Line::from(format!(
            "{} · {} · bake {}",
            if r.managed { "managed" } else { "unmanaged" },
            if r.languages.is_empty() {
                "no manifest".to_string()
            } else {
                r.languages.join(" ")
            },
            bake_word(&r.baked)
        )),
        Line::from(""),
        Line::from("DISPATCHERS"),
    ];
    for (i, n) in DISPATCHERS.iter().enumerate() {
        let s = match r.shims.get(i) {
            Some(ShimState::Ok { baked }) => format!("ok       -> {baked}"),
            Some(ShimState::Drifted) => "DRIFTED  does not match the template".into(),
            _ => "MISSING".to_string(),
        };
        lines.push(Line::from(format!("  {n:<20} {s}")));
    }
    if !r.stale_ours.is_empty() || !r.foreign_subs.is_empty() || r.hook_pkgjson {
        lines.push(Line::from(""));
        lines.push(Line::from("LEFTOVERS (nothing dispatches these)"));
        for n in r.stale_ours.iter().chain(r.foreign_subs.iter()) {
            lines.push(Line::from(format!("  {n}")));
        }
        if r.hook_pkgjson {
            lines.push(Line::from("  package.json (node era)"));
        }
    }
    if !r.skips.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(format!("hook.skip: {}", r.skips.join(", "))));
    }
    f.render_widget(Paragraph::new(lines), area);
}

fn footer(f: &mut Frame, area: Rect, app: &App) {
    let rows = app.rows().len();
    let total = app.scan.repos.len();
    let left = if app.filtering {
        format!("/{}", app.filter)
    } else if app.filter.is_empty() {
        format!("{rows} rows")
    } else {
        // The match count is always visible while filtering, so an empty result
        // is legible as "the filter excluded everything", not as "nothing here".
        format!("{rows} of {total} rows match {:?}", app.filter)
    };
    let keys = if app.detail {
        "esc back  q quit"
    } else {
        "j/k move  enter detail  / filter  esc clear  q quit"
    };
    f.render_widget(Paragraph::new(Line::from(format!("{left}   {keys}"))), area);
}

/// Run the dashboard. Scanning happens on a worker thread so the UI thread
/// never blocks: the scan takes ~7s on a real fleet, well past the point at
/// which an interface stops feeling responsive, and `q` has to work throughout.
pub fn run(root: std::path::PathBuf, depth: usize, binary: String) -> std::io::Result<()> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind};
    use crossterm::terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    };
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    enum Msg {
        Visited(usize),
        /// A repository, sent as it is found. The spec calls streaming
        /// mandatory rather than an optimisation: a spinner over a blank screen
        /// for seven seconds is not "visibility of system status".
        Found(Box<Repo>),
        Done(Box<FleetScan>),
    }

    let (tx, rx) = mpsc::channel();
    let scan_root = root.clone();
    std::thread::spawn(move || {
        let t = tx.clone();
        let scan = crate::scan::scan_with(&scan_root, depth, &binary, &mut |p| match p {
            // Throttled: a message per directory would spend more time in the
            // channel than in the walk.
            crate::scan::Progress::Visited(n) if n % 200 == 0 => {
                let _ = t.send(Msg::Visited(n));
            }
            crate::scan::Progress::Found(r) => {
                let _ = t.send(Msg::Found(Box::new(r.clone())));
            }
            _ => {}
        });
        let _ = tx.send(Msg::Done(Box::new(scan)));
    });

    enable_raw_mode()?;
    let mut out = std::io::stdout();
    crossterm::execute!(out, EnterAlternateScreen)?;
    let mut term = Terminal::new(CrosstermBackend::new(out))?;

    let started = Instant::now();
    let mut app = App::new(FleetScan {
        root,
        depth,
        git_dirs_found: 0,
        hook_dirs_seen: 0,
        managed_seen: 0,
        unmanaged_seen: 0,
        unreadable: Vec::new(),
        excluded_dirs: 0,
        dirs_visited: 0,
        repos: Vec::new(),
    });
    app.scanning = true;

    let result = loop {
        while let Ok(msg) = rx.try_recv() {
            match msg {
                Msg::Visited(n) => app.visited = n,
                Msg::Found(r) => {
                    // Counters are recomputed from what has arrived, so the
                    // header never shows a denominator it cannot justify.
                    app.scan.repos.push(*r);
                    app.scan.git_dirs_found = app.scan.repos.len();
                    app.scan.managed_seen = app.scan.repos.iter().filter(|r| r.managed).count();
                    app.scan.unmanaged_seen = app.scan.git_dirs_found - app.scan.managed_seen;
                }
                Msg::Done(s) => {
                    app.scan = *s;
                    app.scanning = false;
                }
            }
        }
        app.elapsed = started.elapsed().as_secs_f64();
        if let Err(e) = term.draw(|f| draw(f, &app)) {
            break Err(e);
        }
        // 16ms: one frame. Long enough not to spin, short enough that a
        // keystroke never feels dropped.
        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    match k.code {
                        KeyCode::Char(c) => app.on_key(c, false, false, false),
                        KeyCode::Enter => app.on_key('\0', true, false, false),
                        KeyCode::Esc => app.on_key('\0', false, true, false),
                        KeyCode::Backspace => app.on_key('\0', false, false, true),
                        KeyCode::Down => app.on_key('j', false, false, false),
                        KeyCode::Up => app.on_key('k', false, false, false),
                        _ => {}
                    }
                }
            }
        }
        if app.quit {
            break Ok(());
        }
    };

    disable_raw_mode()?;
    crossterm::execute!(term.backend_mut(), LeaveAlternateScreen)?;
    term.show_cursor()?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    fn render(app: &App, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn empty_scan() -> FleetScan {
        FleetScan {
            root: PathBuf::from("/Users/me/Dev"),
            depth: 6,
            git_dirs_found: 0,
            hook_dirs_seen: 0,
            managed_seen: 0,
            unmanaged_seen: 0,
            unreadable: Vec::new(),
            excluded_dirs: 0,
            dirs_visited: 412,
            repos: Vec::new(),
        }
    }

    fn repo(path: &str, managed: bool) -> Repo {
        Repo {
            path: PathBuf::from(path),
            managed,
            shims: vec![
                ShimState::Ok {
                    baked: "/bin/gh".into()
                };
                4
            ],
            baked: BakeState::Current,
            stale_ours: Vec::new(),
            foreign_subs: Vec::new(),
            hook_pkgjson: false,
            languages: vec!["rust".into()],
            skips: Vec::new(),
        }
    }

    fn scan_with_repos(rs: Vec<Repo>) -> FleetScan {
        let managed = rs.iter().filter(|r| r.managed).count();
        FleetScan {
            root: PathBuf::from("/root"),
            depth: 6,
            git_dirs_found: rs.len(),
            hook_dirs_seen: rs.len(),
            managed_seen: managed,
            unmanaged_seen: rs.len() - managed,
            unreadable: Vec::new(),
            excluded_dirs: 3,
            dirs_visited: 100,
            repos: rs,
        }
    }

    /// THE success criterion, from the spec: a broken scan must say so rather
    /// than render a clean, empty, green fleet. This is the whole reason the
    /// tool was worth building, so it is a test and not a promise.
    #[test]
    fn an_empty_scan_renders_scan_failure() {
        let out = render(&App::new(empty_scan()), 90, 16);
        assert!(out.contains("SCAN FAILURE"), "{out}");
        assert!(out.contains("--root"), "must name what to check: {out}");
        assert!(out.contains("--depth"), "{out}");
        assert!(
            out.contains("/Users/me/Dev"),
            "and show the values used: {out}"
        );
        assert!(out.contains("412"), "and what it did look at: {out}");
    }

    /// The inverse: a healthy fleet must NOT shout failure.
    #[test]
    fn a_populated_scan_does_not_claim_failure() {
        let out = render(&App::new(scan_with_repos(vec![repo("a", true)])), 90, 16);
        assert!(!out.contains("SCAN FAILURE"), "{out}");
    }

    /// Counts carry denominators. `4/4`, never "consistent".
    #[test]
    fn the_consistency_band_shows_a_denominator() {
        let app = App::new(scan_with_repos(vec![repo("a", true), repo("b", true)]));
        let out = render(&app, 110, 16);
        assert!(out.contains("2/2"), "expected N/M per hook: {out}");
        assert!(out.contains("repositories"), "{out}");
    }

    #[test]
    fn narrow_terminals_drop_columns_rather_than_scroll() {
        let app = App::new(scan_with_repos(vec![repo("some/repo", true)]));
        let wide = render(&app, 110, 12);
        assert!(wide.contains("LANG") && wide.contains("SKIPS"), "{wide}");
        let mid = render(&app, 80, 12);
        assert!(!mid.contains("LANG"), "LANG should be dropped first: {mid}");
        assert!(mid.contains("SHIMS"), "{mid}");
        let narrow = render(&app, 50, 12);
        assert!(!narrow.contains("SHIMS"), "{narrow}");
        assert!(
            narrow.contains("REPO") && narrow.contains("STATE"),
            "{narrow}"
        );
    }

    /// State is legible without colour: the glyph column and a word.
    #[test]
    fn state_is_encoded_in_text_not_only_colour() {
        let mut r = repo("x", true);
        r.shims[2] = ShimState::Missing;
        let out = render(&App::new(scan_with_repos(vec![r])), 110, 12);
        assert!(out.contains("missing"), "a word, not just a colour: {out}");
        assert!(out.contains('○'), "and a distinct glyph: {out}");
    }

    #[test]
    fn an_unmanaged_repo_is_labelled_not_hidden() {
        let out = render(
            &App::new(scan_with_repos(vec![repo("data/repo", false)])),
            110,
            12,
        );
        assert!(out.contains("unmanaged"), "{out}");
    }

    #[test]
    fn detail_lists_every_dispatcher() {
        let mut app = App::new(scan_with_repos(vec![repo("a", true)]));
        app.detail = true;
        let out = render(&app, 100, 20);
        for n in DISPATCHERS {
            assert!(out.contains(n), "detail must list {n}: {out}");
        }
    }

    #[test]
    fn filtering_shows_the_match_count_against_the_total() {
        let mut app = App::new(scan_with_repos(vec![
            repo("alpha", true),
            repo("beta", true),
            repo("gamma", true),
        ]));
        app.filter = "bet".into();
        assert_eq!(app.rows().len(), 1);
        let out = render(&app, 100, 12);
        assert!(
            out.contains("1 of 3"),
            "an empty result must be legible: {out}"
        );
    }

    #[test]
    fn keys_move_enter_and_quit() {
        let mut app = App::new(scan_with_repos(vec![repo("a", true), repo("b", true)]));
        app.on_key('j', false, false, false);
        assert_eq!(app.selected, 1);
        app.on_key('j', false, false, false);
        assert_eq!(app.selected, 1, "must not run past the end");
        app.on_key('k', false, false, false);
        assert_eq!(app.selected, 0);
        app.on_key('\0', true, false, false);
        assert!(app.detail);
        app.on_key('\0', false, true, false);
        assert!(!app.detail, "esc leaves detail before clearing a filter");
        app.on_key('q', false, false, false);
        assert!(app.quit);
    }

    #[test]
    fn filter_mode_captures_typing_and_escape_clears_it() {
        let mut app = App::new(scan_with_repos(vec![repo("alpha", true)]));
        app.on_key('/', false, false, false);
        assert!(app.filtering);
        for c in "alp".chars() {
            app.on_key(c, false, false, false);
        }
        assert_eq!(app.filter, "alp");
        app.on_key('\0', false, false, true);
        assert_eq!(app.filter, "al", "backspace edits rather than exits");
        app.on_key('\0', false, true, false);
        assert!(!app.filtering && app.filter.is_empty());
    }
}
