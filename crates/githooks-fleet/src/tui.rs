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

use crate::checks;
use crate::scan::{FleetScan, Repo};
use crate::shim::{BakeState, ShimState, DISPATCHERS};

/// What the screen is showing, and what a keystroke therefore means.
///
/// Previously this was four independent booleans — `detail`, `hook_view`,
/// `filtering`, plus `scanning` — which describes sixteen states of which four
/// are meaningful. Adding a command palette and a type-to-confirm prompt would
/// have taken it to sixty-four. Making the invalid combinations unrepresentable
/// is what keeps the next two features testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Browse,
    Detail,
    HookView,
    /// Typing into the filter. The row list narrows as you type.
    Filter,
}

/// A keystroke, named. The previous signature was
/// `on_key(char, bool, bool, bool)`, where a caller had to remember that the
/// second bool meant Enter, and `'\0'` meant "not a character".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Esc,
    Backspace,
    Up,
    Down,
}

/// One line of editable text with a cursor at the end.
///
/// Extracted because the filter, the command palette and the type-the-name
/// confirmation are the same thing wearing different prompts. Building it once
/// is the point of this refactor.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LineEdit {
    value: String,
}

impl LineEdit {
    pub fn insert(&mut self, c: char) {
        self.value.push(c);
    }
    pub fn backspace(&mut self) {
        self.value.pop();
    }
    pub fn clear(&mut self) {
        self.value.clear();
    }
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }
    pub fn as_str(&self) -> &str {
        &self.value
    }
    /// Test-only until a real caller exists. The palette and the
    /// type-to-confirm prompt will want it; shipping it as public dead code on
    /// that promise is how `Progress::Found` sat unused while pretending the
    /// spec was implemented.
    #[cfg(test)]
    pub fn set(&mut self, v: &str) {
        self.value = v.to_string();
    }
}

pub struct App {
    pub scan: FleetScan,
    pub selected: usize,
    pub mode: Mode,
    /// Where Esc returns to when a prompt is dismissed.
    prev_mode: Mode,
    pub filter: LineEdit,
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
            mode: Mode::Browse,
            prev_mode: Mode::Browse,
            filter: LineEdit::default(),
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
                        .contains(&self.filter.as_str().to_lowercase())
            })
            .collect()
    }

    pub fn on_key(&mut self, key: Key) {
        match self.mode {
            Mode::Filter => self.filter_key(key),
            _ => self.browse_key(key),
        }
    }

    fn filter_key(&mut self, key: Key) {
        match key {
            Key::Esc => {
                self.filter.clear();
                self.mode = self.prev_mode;
            }
            // Enter keeps the filter and leaves the prompt.
            Key::Enter => self.mode = self.prev_mode,
            Key::Backspace => self.filter.backspace(),
            Key::Char(c) => self.filter.insert(c),
            Key::Up | Key::Down => {}
        }
        self.selected = 0;
    }

    fn browse_key(&mut self, key: Key) {
        let len = self.rows().len();
        match key {
            Key::Char('q') => self.quit = true,
            Key::Char('/') => {
                self.prev_mode = self.mode;
                self.mode = Mode::Filter;
            }
            Key::Char('h') => {
                self.mode = if self.mode == Mode::HookView {
                    Mode::Browse
                } else {
                    Mode::HookView
                }
            }
            Key::Char('j') | Key::Down if len > 0 => {
                self.selected = (self.selected + 1).min(len - 1)
            }
            Key::Char('k') | Key::Up => self.selected = self.selected.saturating_sub(1),
            Key::Enter if len > 0 => self.mode = Mode::Detail,
            Key::Esc => {
                // Leave the current screen first; only then clear a filter.
                if self.mode != Mode::Browse {
                    self.mode = Mode::Browse;
                } else {
                    self.filter.clear();
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
    if app.mode == Mode::HookView {
        hooks_view(f, chunks[1], app);
    } else if app.mode == Mode::Detail {
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

/// The transposed matrix: checks down the side, repo counts across.
///
/// Answers "where does this check actually apply?", which the old text output
/// could not. `APPLICABLE = ACTIVE + SKIPPED`, and INERT is counted separately
/// because a check that is correctly silent is not a problem — conflating the
/// two would invent ninety false problems out of the Rust checks alone.
fn hooks_view(f: &mut Frame, area: Rect, app: &App) {
    let rows = checks::rollup(&app.scan.repos);
    let managed = app.scan.managed_seen;
    let body: Vec<Row> = rows
        .iter()
        .map(|r| {
            // A check that can never fire anywhere is either dead or
            // misconfigured, and that is invisible in a plain list.
            let flag = if r.applicable == 0 { "  <- never" } else { "" };
            Row::new(vec![
                Cell::from(format!("{}{flag}", r.name)),
                Cell::from(format!("{}/{managed}", r.applicable)),
                Cell::from(r.active.to_string()),
                Cell::from(r.skipped.to_string()),
                Cell::from(r.inert.to_string()),
            ])
        })
        .collect();
    f.render_widget(
        Table::new(
            body,
            [
                Constraint::Min(30),
                Constraint::Length(12),
                Constraint::Length(8),
                Constraint::Length(9),
                Constraint::Length(7),
            ],
        )
        .header(Row::new(vec![
            "CHECK",
            "APPLICABLE",
            "ACTIVE",
            "SKIPPED",
            "INERT",
        ])),
        area,
    );
}

fn footer(f: &mut Frame, area: Rect, app: &App) {
    let rows = app.rows().len();
    let total = app.scan.repos.len();
    let left = if app.mode == Mode::Filter {
        format!("/{}", app.filter.as_str())
    } else if app.filter.is_empty() {
        format!("{rows} rows")
    } else {
        // The match count is always visible while filtering, so an empty result
        // is legible as "the filter excluded everything", not as "nothing here".
        format!("{rows} of {total} rows match {:?}", app.filter.as_str())
    };
    let keys = if app.mode == Mode::HookView {
        "h fleet  q quit"
    } else if app.mode == Mode::Detail {
        "esc back  q quit"
    } else {
        "j/k move  enter detail  / filter  h hooks  esc clear  q quit"
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
                    // crossterm's key codes map straight onto ours; arrows are
                    // their own variants rather than being spelled as j/k, so
                    // they keep working inside a text prompt.
                    let mapped = match k.code {
                        KeyCode::Char(c) => Some(Key::Char(c)),
                        KeyCode::Enter => Some(Key::Enter),
                        KeyCode::Esc => Some(Key::Esc),
                        KeyCode::Backspace => Some(Key::Backspace),
                        KeyCode::Down => Some(Key::Down),
                        KeyCode::Up => Some(Key::Up),
                        _ => None,
                    };
                    if let Some(key) = mapped {
                        app.on_key(key);
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
        app.mode = Mode::Detail;
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
        app.filter.set("bet");
        assert_eq!(app.rows().len(), 1);
        let out = render(&app, 100, 12);
        assert!(
            out.contains("1 of 3"),
            "an empty result must be legible: {out}"
        );
    }

    #[test]
    fn the_hook_view_transposes_and_keeps_a_denominator() {
        let mut app = App::new(scan_with_repos(vec![repo("a", true), repo("b", true)]));
        app.mode = Mode::HookView;
        let out = render(&app, 110, 30);
        assert!(out.contains("APPLICABLE"), "{out}");
        assert!(out.contains("INERT"), "inert must be its own column: {out}");
        assert!(out.contains("pre-commit-clippy"), "{out}");
        assert!(out.contains("2/2"), "counts carry the managed total: {out}");
    }

    /// A check that applies nowhere is called out, because "0 everywhere" is
    /// invisible in a column of numbers.
    #[test]
    fn a_check_that_applies_nowhere_is_flagged() {
        let mut r = repo("only-js", true);
        r.languages = vec!["js".into()];
        let mut app = App::new(scan_with_repos(vec![r]));
        app.mode = Mode::HookView;
        let out = render(&app, 110, 30);
        assert!(
            out.contains("never"),
            "expected a marker on the dead rows: {out}"
        );
    }

    #[test]
    fn h_toggles_the_hook_view() {
        let mut app = App::new(scan_with_repos(vec![repo("a", true)]));
        assert_eq!(app.mode, Mode::Browse);
        app.on_key(Key::Char('h'));
        assert_eq!(app.mode, Mode::HookView);
        app.on_key(Key::Char('h'));
        assert_eq!(app.mode, Mode::Browse);
    }
    #[test]
    fn keys_move_enter_and_quit() {
        let mut app = App::new(scan_with_repos(vec![repo("a", true), repo("b", true)]));
        app.on_key(Key::Char('j'));
        assert_eq!(app.selected, 1);
        app.on_key(Key::Char('j'));
        assert_eq!(app.selected, 1, "must not run past the end");
        app.on_key(Key::Char('k'));
        assert_eq!(app.selected, 0);
        app.on_key(Key::Enter);
        assert_eq!(app.mode, Mode::Detail);
        app.on_key(Key::Esc);
        assert_eq!(
            app.mode,
            Mode::Browse,
            "esc leaves detail before clearing a filter"
        );
        app.on_key(Key::Char('q'));
        assert!(app.quit);
    }

    /// Arrows are their own variants rather than aliases for j/k, so they still
    /// move the selection but do not type letters into a prompt.
    #[test]
    fn arrows_move_and_do_not_type() {
        let mut app = App::new(scan_with_repos(vec![repo("a", true), repo("b", true)]));
        app.on_key(Key::Down);
        assert_eq!(app.selected, 1);
        app.on_key(Key::Up);
        assert_eq!(app.selected, 0);

        app.on_key(Key::Char('/'));
        app.on_key(Key::Down);
        assert_eq!(
            app.filter.as_str(),
            "",
            "an arrow must not become a character"
        );
    }

    #[test]
    fn filter_mode_captures_typing_and_escape_clears_it() {
        let mut app = App::new(scan_with_repos(vec![repo("alpha", true)]));
        app.on_key(Key::Char('/'));
        assert_eq!(app.mode, Mode::Filter);
        for c in "alp".chars() {
            app.on_key(Key::Char(c));
        }
        assert_eq!(app.filter.as_str(), "alp");
        app.on_key(Key::Backspace);
        assert_eq!(
            app.filter.as_str(),
            "al",
            "backspace edits rather than exits"
        );
        app.on_key(Key::Esc);
        assert_eq!(app.mode, Mode::Browse);
        assert!(app.filter.is_empty());
    }

    /// `q` inside a prompt is a letter, not a command. Losing this is the
    /// classic modal-editor bug.
    #[test]
    fn q_types_rather_than_quits_while_filtering() {
        let mut app = App::new(scan_with_repos(vec![repo("a", true)]));
        app.on_key(Key::Char('/'));
        app.on_key(Key::Char('q'));
        assert!(!app.quit, "q must be text here");
        assert_eq!(app.filter.as_str(), "q");
    }

    /// Enter keeps the filter and returns to the list; Esc discards it. Two
    /// different intentions that a single "leave the prompt" would conflate.
    #[test]
    fn enter_keeps_the_filter_and_esc_discards_it() {
        let mut app = App::new(scan_with_repos(vec![
            repo("alpha", true),
            repo("beta", true),
        ]));
        app.on_key(Key::Char('/'));
        app.on_key(Key::Char('b'));
        app.on_key(Key::Enter);
        assert_eq!(app.mode, Mode::Browse);
        assert_eq!(app.filter.as_str(), "b", "enter commits the filter");
        assert_eq!(app.rows().len(), 1);
    }
}
