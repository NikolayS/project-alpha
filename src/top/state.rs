//! `/top` UI state — pure data, no I/O.
//!
//! The state machine is intentionally tiny in S1: a single Activity view, a
//! row cursor, an optional last-error string, and an exit flag. Later sprints
//! will extend `View`, add filter/sort, and grow the ring buffer for
//! pause-and-rewind. All UI rendering reads from this struct; it should
//! remain the single source of truth.
//!
//! S1 keys handled by [`App::handle_key`]:
//!   - `q`, `Esc`, `Ctrl-C`         → exit
//!   - `Up` / `k`, `Down` / `j`     → move row cursor
//!   - `PageUp` / `PageDown`        → jump cursor by page
//!   - `Home` / `End`               → first / last row
//!
//! Any other key is ignored at this stage.

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

/// UI state for `/top`. Pure data — no I/O, no side effects.
#[derive(Debug, Default)]
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

    /// Jump cursor to the first row.
    pub fn cursor_home(&mut self) {
        self.selected_row = 0;
    }

    /// Jump cursor to the last row.
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

    /// Note a missed (timed-out) tick.
    pub fn note_stale(&mut self) {
        self.stale_ticks = self.stale_ticks.saturating_add(1);
    }

    /// Note a sampler error. Caller should keep the message short.
    pub fn note_error(&mut self, msg: String) {
        self.last_error = Some(msg);
    }

    /// Process a key event. Returns `true` when the caller should exit the
    /// event loop. The exit signal is also stored in `should_exit` so
    /// renderers/tests can observe it.
    pub fn handle_key(&mut self, key: KeyEvent, page_size: usize) -> bool {
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
            _ => {}
        }
        self.should_exit
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

        // Down 1 from 0
        app.handle_key(key(KeyCode::Down), 10);
        assert_eq!(app.selected_row, 1);

        // Down past the end stays at last row
        for _ in 0..50 {
            app.handle_key(key(KeyCode::Down), 10);
        }
        assert_eq!(app.selected_row, 4);

        // Up past start stays at 0
        for _ in 0..50 {
            app.handle_key(key(KeyCode::Up), 10);
        }
        assert_eq!(app.selected_row, 0);

        // Home / End
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

        // Row count shrinks; cursor must clamp to the new last row.
        app.set_snapshot(snap_with(3));
        assert_eq!(app.selected_row, 2);

        // And to 0 if there are no rows at all.
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
}
