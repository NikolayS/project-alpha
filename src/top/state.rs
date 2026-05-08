//! `/top` UI state — pure data, no I/O.
//!
//! All UI rendering reads from this struct; it is the single source of
//! truth. Later sprints will extend `View`, add filter/sort, and grow the
//! ring buffer for pause-and-rewind.
//!
//! S1 keys handled by [`App::handle_key`]:
//!   - `q`, `Esc`, `Ctrl-C`         → exit (Esc cancels an open prompt first)
//!   - `Up` / `k`, `Down` / `j`     → move row cursor (sticky-header scroll)
//!   - `PageUp` / `PageDown`        → jump cursor by page
//!   - `Home` / `End`               → first / last row
//!   - `s`                          → set refresh delay (prompt 0.1–60 s)
//!   - `e`                          → toggle extended columns (app/client/backend)
//!
//! When a prompt is open, every other key feeds it (digits, `.`, Backspace,
//! Enter to apply, Esc to cancel).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// A single backend row drawn in the Activity view.
///
/// Field types mirror what the sampler decodes from `pg_stat_activity`. We
/// keep numbers as native widths so renderer and tests can reason about them
/// without re-parsing.
#[derive(Debug, Clone, Default)]
pub struct ActivityRow {
    pub pid: i32,
    pub usename: String,
    pub datname: String,
    pub application_name: String,
    pub client_addr: String,
    pub backend_type: String,
    pub state: String,
    /// `wait_event_type` from `pg_stat_activity` (e.g. `"IO"`, `"Lock"`,
    /// `"LWLock"`). Empty when the backend is on CPU.
    pub wait_event_type: String,
    pub wait_event: String,
    /// Seconds elapsed since `query_start`. `None` when the backend is idle.
    pub qtime_secs: Option<f64>,
    /// Seconds elapsed since `xact_start`. `None` when the backend is idle.
    pub xtime_secs: Option<f64>,
    /// Trimmed query text (already truncated by the sampler to keep snapshots
    /// small). The renderer truncates further to the available column width.
    pub query: String,
    /// Number of granted locks held by this backend (count from `pg_locks`).
    /// Includes virtualxid / transactionid locks every active backend has.
    pub locks_held: i64,
}

/// Server-wide summary drawn in the header bar.
#[derive(Debug, Clone, Default)]
pub struct ServerSummary {
    pub db_name: String,
    pub user: String,
    pub pg_version: String,
    /// Uptime in seconds since postmaster start.
    pub uptime_secs: i64,
    pub in_recovery: bool,
    pub active: u32,
    pub idle_in_tx: u32,
    pub waiting: u32,
    pub total_backends: u32,
    pub max_connections: u32,
    /// Age of the longest-running open transaction (any state). 0 when no
    /// backend is in a transaction.
    pub longest_xact_secs: f64,
    /// Age of the longest-running *active* query (excludes idle backends).
    /// 0 when no backend is currently running a query.
    pub longest_active_query_secs: f64,
    /// Cumulative deadlocks across every database in the cluster (sum of
    /// `pg_stat_database.deadlocks`).
    pub deadlocks_total: i64,
    /// Cumulative temp file count across every database in the cluster.
    pub temp_files_total: i64,
}

/// One sample tick of data; what the sampler produces and the renderer reads.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// Unix timestamp (seconds) when the sample was taken.
    pub ts: i64,
    pub server: ServerSummary,
    pub rows: Vec<ActivityRow>,
}

/// Top-level view selector. S1 ships only `Activity`; later sprints add the
/// remaining pgcenter-style views (databases, tables, indexes, statements,
/// replication, progress, wal, functions, blocking).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Activity,
}

impl View {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Activity => "Activity",
        }
    }
}

/// Footer prompt state. `s` opens the refresh-delay prompt; later sprints
/// will reuse this struct for the filter (`/`) and sort (`o`) prompts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptState {
    pub kind: PromptKind,
    pub buffer: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// Set refresh interval in seconds.
    Refresh,
}

impl PromptKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Refresh => "delay (secs)",
        }
    }
}

/// Default refresh interval in seconds when neither the CLI flag nor the
/// runtime prompt overrides it.
pub const DEFAULT_REFRESH_SECS: f64 = 1.0;

/// Lower / upper bounds for the refresh prompt. 100 ms keeps the sampler
/// from monopolising the connection; 60 s avoids the user accidentally
/// disabling refresh entirely.
pub const MIN_REFRESH_SECS: f64 = 0.1;
pub const MAX_REFRESH_SECS: f64 = 60.0;

/// UI state for `/top`. Pure data — no I/O, no side effects.
#[derive(Debug)]
pub struct App {
    pub view: View,
    pub snapshot: Option<Snapshot>,
    /// Index into `snapshot.rows`; clamped on every render to remain valid
    /// when row counts shrink between ticks.
    pub selected_row: usize,
    /// Last sampler error message, surfaced in the footer until the next
    /// successful tick clears it. Kept short — multi-line errors are
    /// truncated by the renderer.
    pub last_error: Option<String>,
    /// Number of consecutive sampler ticks that timed out. Surfaces a brief
    /// "stale Ns" badge in the header until a fresh tick lands.
    pub stale_ticks: u32,
    /// Set by [`App::handle_key`] when the user requests exit.
    pub should_exit: bool,
    /// Sampler refresh interval in seconds.
    pub refresh_secs: f64,
    /// When `true`, the activity table renders `app`, `client`, and
    /// `backend` columns in addition to the default set. Toggled by `e`.
    pub extended: bool,
    /// Footer prompt state. `Some` when the user is typing into a prompt.
    pub prompt: Option<PromptState>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            view: View::default(),
            snapshot: None,
            selected_row: 0,
            last_error: None,
            stale_ticks: 0,
            should_exit: false,
            refresh_secs: DEFAULT_REFRESH_SECS,
            extended: false,
            prompt: None,
        }
    }
}

impl App {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of visible rows in the active view.
    pub fn row_count(&self) -> usize {
        self.snapshot.as_ref().map_or(0, |snap| snap.rows.len())
    }

    /// Move the cursor up by `n`, saturating at 0.
    pub fn cursor_up(&mut self, n: usize) {
        self.selected_row = self.selected_row.saturating_sub(n);
    }

    /// Move the cursor down by `n`, clamped to `row_count() - 1`.
    pub fn cursor_down(&mut self, n: usize) {
        let max = self.row_count().saturating_sub(1);
        self.selected_row = self.selected_row.saturating_add(n).min(max);
    }

    pub fn cursor_home(&mut self) {
        self.selected_row = 0;
    }

    pub fn cursor_end(&mut self) {
        self.selected_row = self.row_count().saturating_sub(1);
    }

    /// Clamp `selected_row` to be a valid index for the current row count.
    /// Called after every snapshot replace so the renderer never reads OOB.
    pub fn clamp_cursor(&mut self) {
        let max = self.row_count().saturating_sub(1);
        if self.selected_row > max {
            self.selected_row = max;
        }
    }

    /// Replace the current snapshot and reset stale-tick counter.
    pub fn set_snapshot(&mut self, snap: Snapshot) {
        self.snapshot = Some(snap);
        self.stale_ticks = 0;
        self.last_error = None;
        self.clamp_cursor();
    }

    pub fn note_stale(&mut self) {
        self.stale_ticks = self.stale_ticks.saturating_add(1);
    }

    pub fn note_error(&mut self, msg: String) {
        self.last_error = Some(msg);
    }

    /// Open the refresh-delay prompt seeded with the current value.
    pub fn open_refresh_prompt(&mut self) {
        self.prompt = Some(PromptState {
            kind: PromptKind::Refresh,
            buffer: format_refresh_seed(self.refresh_secs),
        });
    }

    /// Apply the currently open prompt, parsing its buffer and updating the
    /// corresponding setting. Out-of-range or unparseable input is ignored
    /// (the prompt closes either way).
    pub fn apply_prompt(&mut self) {
        if let Some(prompt) = self.prompt.take() {
            match prompt.kind {
                PromptKind::Refresh => {
                    if let Ok(n) = prompt.buffer.parse::<f64>() {
                        if (MIN_REFRESH_SECS..=MAX_REFRESH_SECS).contains(&n) {
                            self.refresh_secs = n;
                        }
                    }
                }
            }
        }
    }

    /// Process a key event. Returns `true` when the caller should exit the
    /// event loop. The exit signal is also stored in `should_exit` so
    /// renderers/tests can observe it.
    pub fn handle_key(&mut self, key: KeyEvent, page_size: usize) -> bool {
        // Active prompt swallows almost every key.
        if self.prompt.is_some() {
            self.handle_prompt_key(key);
            return self.should_exit;
        }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_exit = true;
            }
            KeyCode::Char('c') if ctrl => {
                self.should_exit = true;
            }
            KeyCode::Up | KeyCode::Char('k') => self.cursor_up(1),
            KeyCode::Down | KeyCode::Char('j') => self.cursor_down(1),
            KeyCode::PageUp => self.cursor_up(page_size.max(1)),
            KeyCode::PageDown => self.cursor_down(page_size.max(1)),
            KeyCode::Home => self.cursor_home(),
            KeyCode::End => self.cursor_end(),
            KeyCode::Char('s') => self.open_refresh_prompt(),
            KeyCode::Char('e') => self.extended = !self.extended,
            _ => {}
        }
        self.should_exit
    }

    fn handle_prompt_key(&mut self, key: KeyEvent) {
        let Some(prompt) = self.prompt.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.prompt = None;
            }
            KeyCode::Enter => self.apply_prompt(),
            KeyCode::Backspace => {
                prompt.buffer.pop();
            }
            // Reasonable upper bound on prompt length keeps the buffer bounded.
            KeyCode::Char(c) if (c.is_ascii_digit() || c == '.') && prompt.buffer.len() < 10 => {
                prompt.buffer.push(c);
            }
            _ => {}
        }
    }
}

fn format_refresh_seed(secs: f64) -> String {
    if (secs - secs.round()).abs() < 0.0005 {
        format!("{secs:.0}")
    } else {
        format!("{secs:.2}")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn snap_with(n: usize) -> Snapshot {
        Snapshot {
            ts: 1_700_000_000,
            server: ServerSummary {
                db_name: "prod".into(),
                user: "nik".into(),
                pg_version: "16.4".into(),
                ..Default::default()
            },
            rows: (0..n)
                .map(|i| ActivityRow {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                    pid: 10_000 + i as i32,
                    usename: "app".into(),
                    datname: "prod".into(),
                    state: "active".into(),
                    query: format!("select {i}"),
                    ..Default::default()
                })
                .collect(),
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn new_app_has_empty_snapshot_and_zero_cursor() {
        let app = App::new();
        assert!(app.snapshot.is_none());
        assert_eq!(app.selected_row, 0);
        assert_eq!(app.row_count(), 0);
        assert!(!app.should_exit);
        assert_eq!(app.view, View::Activity);
        assert!((app.refresh_secs - DEFAULT_REFRESH_SECS).abs() < f64::EPSILON);
        assert!(!app.extended);
        assert!(app.prompt.is_none());
    }

    #[test]
    fn q_esc_and_ctrl_c_request_exit() {
        for k in [key(KeyCode::Char('q')), key(KeyCode::Esc), ctrl('c')] {
            let mut app = App::new();
            assert!(app.handle_key(k, 10));
            assert!(app.should_exit);
        }
    }

    #[test]
    fn cursor_navigation_clamps_to_bounds() {
        let mut app = App::new();
        app.set_snapshot(snap_with(5));

        app.handle_key(key(KeyCode::Down), 10);
        assert_eq!(app.selected_row, 1);
        for _ in 0..50 {
            app.handle_key(key(KeyCode::Down), 10);
        }
        assert_eq!(app.selected_row, 4);
        for _ in 0..50 {
            app.handle_key(key(KeyCode::Up), 10);
        }
        assert_eq!(app.selected_row, 0);

        app.handle_key(key(KeyCode::End), 10);
        assert_eq!(app.selected_row, 4);
        app.handle_key(key(KeyCode::Home), 10);
        assert_eq!(app.selected_row, 0);
    }

    #[test]
    fn page_up_down_uses_provided_page_size() {
        let mut app = App::new();
        app.set_snapshot(snap_with(20));
        app.handle_key(key(KeyCode::PageDown), 7);
        assert_eq!(app.selected_row, 7);
        app.handle_key(key(KeyCode::PageDown), 7);
        assert_eq!(app.selected_row, 14);
        app.handle_key(key(KeyCode::PageUp), 5);
        assert_eq!(app.selected_row, 9);
    }

    #[test]
    fn vim_style_j_k_move_cursor() {
        let mut app = App::new();
        app.set_snapshot(snap_with(3));
        app.handle_key(key(KeyCode::Char('j')), 10);
        app.handle_key(key(KeyCode::Char('j')), 10);
        assert_eq!(app.selected_row, 2);
        app.handle_key(key(KeyCode::Char('k')), 10);
        assert_eq!(app.selected_row, 1);
    }

    #[test]
    fn snapshot_replace_clamps_cursor() {
        let mut app = App::new();
        app.set_snapshot(snap_with(10));
        app.handle_key(key(KeyCode::End), 10);
        assert_eq!(app.selected_row, 9);

        app.set_snapshot(snap_with(3));
        assert_eq!(app.selected_row, 2);

        app.set_snapshot(snap_with(0));
        assert_eq!(app.selected_row, 0);
    }

    #[test]
    fn note_stale_increments_and_set_snapshot_clears() {
        let mut app = App::new();
        app.note_stale();
        app.note_stale();
        assert_eq!(app.stale_ticks, 2);
        app.set_snapshot(snap_with(1));
        assert_eq!(app.stale_ticks, 0);
        assert!(app.last_error.is_none());
    }

    #[test]
    fn note_error_round_trip() {
        let mut app = App::new();
        app.note_error("boom".into());
        assert_eq!(app.last_error.as_deref(), Some("boom"));
    }

    #[test]
    fn unknown_keys_are_no_ops() {
        let mut app = App::new();
        app.set_snapshot(snap_with(3));
        let exit = app.handle_key(key(KeyCode::Char('z')), 10);
        assert!(!exit);
        assert_eq!(app.selected_row, 0);
    }

    #[test]
    fn view_label_is_user_visible_text() {
        assert_eq!(View::Activity.label(), "Activity");
    }

    #[test]
    fn e_toggles_extended_columns() {
        let mut app = App::new();
        assert!(!app.extended);
        app.handle_key(key(KeyCode::Char('e')), 10);
        assert!(app.extended);
        app.handle_key(key(KeyCode::Char('e')), 10);
        assert!(!app.extended);
    }

    #[test]
    fn s_opens_refresh_prompt_seeded_with_current_value() {
        let mut app = App::new();
        app.refresh_secs = 1.0;
        app.handle_key(key(KeyCode::Char('s')), 10);
        let prompt = app.prompt.as_ref().expect("prompt opened");
        assert_eq!(prompt.kind, PromptKind::Refresh);
        assert_eq!(prompt.buffer, "1");

        // The fractional seed format keeps two decimals.
        app.prompt = None;
        app.refresh_secs = 0.5;
        app.handle_key(key(KeyCode::Char('s')), 10);
        assert_eq!(app.prompt.as_ref().unwrap().buffer, "0.50");
    }

    #[test]
    fn refresh_prompt_accepts_digits_and_dot() {
        let mut app = App::new();
        app.open_refresh_prompt();
        // Seed is "1"; clear it then type 0.25
        for _ in 0..3 {
            app.handle_key(key(KeyCode::Backspace), 10);
        }
        for c in "0.25".chars() {
            app.handle_key(key(KeyCode::Char(c)), 10);
        }
        assert_eq!(app.prompt.as_ref().unwrap().buffer, "0.25");

        // Letters are rejected.
        app.handle_key(key(KeyCode::Char('x')), 10);
        assert_eq!(app.prompt.as_ref().unwrap().buffer, "0.25");

        app.handle_key(key(KeyCode::Enter), 10);
        assert!(app.prompt.is_none());
        assert!((app.refresh_secs - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn refresh_prompt_clamps_out_of_range_input() {
        let mut app = App::new();
        let original = app.refresh_secs;
        app.open_refresh_prompt();
        for _ in 0..app.prompt.as_ref().unwrap().buffer.len() {
            app.handle_key(key(KeyCode::Backspace), 10);
        }
        for c in "999".chars() {
            app.handle_key(key(KeyCode::Char(c)), 10);
        }
        app.handle_key(key(KeyCode::Enter), 10);
        // 999 > MAX → ignored.
        assert!((app.refresh_secs - original).abs() < f64::EPSILON);
    }

    #[test]
    fn esc_cancels_prompt_without_changing_setting() {
        let mut app = App::new();
        let original = app.refresh_secs;
        app.open_refresh_prompt();
        for c in "0.5".chars() {
            app.handle_key(key(KeyCode::Char(c)), 10);
        }
        // Esc closes the prompt without applying.
        assert!(!app.handle_key(key(KeyCode::Esc), 10));
        assert!(app.prompt.is_none());
        assert!((app.refresh_secs - original).abs() < f64::EPSILON);
        // First Esc closed the prompt; second Esc exits.
        assert!(app.handle_key(key(KeyCode::Esc), 10));
    }

    #[test]
    fn exit_keys_swallowed_while_prompt_open() {
        // q while the prompt is open is treated as a typed character (rejected
        // because it is not a digit or dot), not as an exit signal.
        let mut app = App::new();
        app.open_refresh_prompt();
        let exit = app.handle_key(key(KeyCode::Char('q')), 10);
        assert!(!exit);
        assert!(app.prompt.is_some());
    }
}
