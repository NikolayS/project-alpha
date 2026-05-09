//! `/top` UI state — pure data, no I/O.
//!
//! All UI rendering reads from this struct; it is the single source of
//! truth. Later sprints will extend `View`, add filter/sort, and grow the
//! ring buffer for pause-and-rewind.
//!
//! S1 keys handled by [`App::handle_key`]:
//! - `q`, `Esc`, `Ctrl-C` — exit (Esc cancels an open prompt first)
//! - `Up` / `Down` — move row cursor (sticky-header scroll)
//! - `PageUp` / `PageDown` — jump cursor by page
//! - `Home` / `End` — first / last row
//! - `Space` — force an immediate sampler tick
//! - `←` / `→` (also `<` / `>`) — cycle active sort column
//! - `r` — reverse sort direction
//! - `e` — toggle extended columns (app/client/backend)
//! - `s` — set refresh delay (prompt 0.1–60 s)
//! - `k` / `K` — cancel / terminate selected backend (footer y/N confirm)
//!
//! When a prompt is open, every other key feeds it (digits, `.`, Backspace,
//! Enter to apply, Esc to cancel). When the kill confirm is open, `y`/`Y`
//! fires; anything else cancels.

use std::time::{Duration, Instant};

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
    /// Number of backends with `backend_type = 'autovacuum worker'`.
    pub autovacuum_busy: u32,
    /// `current_setting('autovacuum_max_workers')` — slot ceiling.
    pub autovacuum_max: u32,
    /// Physical replication slot counts (active out of total).
    pub phys_slots: u32,
    pub phys_slots_active: u32,
    /// Logical replication slot counts.
    pub log_slots: u32,
    pub log_slots_active: u32,
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

/// Action requested via `k` (cancel) or `K` (terminate). Cancel sends
/// `pg_cancel_backend` (signals the running query); Terminate sends
/// `pg_terminate_backend` (closes the connection — heavier weapon).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillMode {
    Cancel,
    Terminate,
}

impl KillMode {
    pub const fn verb_upper(self) -> &'static str {
        match self {
            Self::Cancel => "CANCEL",
            Self::Terminate => "TERMINATE",
        }
    }

    pub const fn pg_function(self) -> &'static str {
        match self {
            Self::Cancel => "pg_cancel_backend",
            Self::Terminate => "pg_terminate_backend",
        }
    }
}

/// Snapshot of an [`ActivityRow`] taken at the moment `k`/`K` was pressed.
/// Carrying the row data through the confirmation cycle means the prompt
/// describes the exact backend the user clicked on, even if the table
/// re-sorts under them between the keystroke and the `y` confirmation.
#[derive(Debug, Clone)]
pub struct KillRequest {
    pub mode: KillMode,
    pub pid: i32,
    pub usename: String,
    pub datname: String,
    pub state: String,
    pub qtime_secs: Option<f64>,
    pub query_summary: String,
}

impl KillRequest {
    /// Convenience: forward the mode's PG function name so callers
    /// can build the SQL without re-matching on `KillMode`.
    pub const fn pg_function_for_request(&self) -> &'static str {
        self.mode.pg_function()
    }

    fn from_row(mode: KillMode, row: &ActivityRow) -> Self {
        let mut summary: String = row.query.split_whitespace().collect::<Vec<_>>().join(" ");
        if summary.chars().count() > 60 {
            summary = summary.chars().take(59).collect::<String>();
            summary.push('…');
        }
        Self {
            mode,
            pid: row.pid,
            usename: row.usename.clone(),
            datname: row.datname.clone(),
            state: row.state.clone(),
            qtime_secs: row.qtime_secs,
            query_summary: summary,
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

/// How long a key-press overlay stays visible after the keystroke. Long
/// enough that a viewer of a recorded demo can read the label; short
/// enough to feel ephemeral during interactive use.
pub const KEY_OVERLAY_TTL: Duration = Duration::from_millis(1_200);

/// Sortable columns in the Activity view. Cycled left/right with `<` /
/// `>`; direction toggled with `r`. Default = `Qtime` descending (matches
/// the SQL `order by` and what an operator usually wants in an incident).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Pid,
    User,
    Db,
    State,
    Wait,
    Qtime,
    Xtime,
    Locks,
    Query,
}

impl SortColumn {
    /// Stable canonical ordering used by the `<` / `>` cyclers.
    pub const ALL: &'static [Self] = &[
        Self::Pid,
        Self::User,
        Self::Db,
        Self::State,
        Self::Wait,
        Self::Qtime,
        Self::Xtime,
        Self::Locks,
        Self::Query,
    ];

    /// Lower-case header label that appears in the table.
    pub const fn header_label(self) -> &'static str {
        match self {
            Self::Pid => "pid",
            Self::User => "user",
            Self::Db => "db",
            Self::State => "state",
            Self::Wait => "wait",
            Self::Qtime => "qtime",
            Self::Xtime => "xtime",
            Self::Locks => "locks",
            Self::Query => "query",
        }
    }

    /// Default sort direction when this column is first selected. Most
    /// numeric / time columns are most useful in descending order
    /// (longest first); textual columns are most useful ascending (A→Z).
    pub const fn default_desc(self) -> bool {
        matches!(self, Self::Qtime | Self::Xtime | Self::Locks | Self::Pid)
    }

    fn position(self) -> usize {
        Self::ALL
            .iter()
            .position(|s| *s == self)
            .expect("ALL contains every SortColumn variant")
    }

    /// Step `delta` positions through `Self::ALL` (wrapping). `delta` is
    /// signed so callers can pass `-1` (`<`) or `+1` (`>`); we lift the
    /// modular arithmetic into `usize` after offsetting by `len` to avoid
    /// any signed-cast lossiness clippy might flag.
    fn step(self, delta: isize) -> Self {
        let len = Self::ALL.len();
        let pos = self.position();
        let stepped = if delta >= 0 {
            #[allow(clippy::cast_sign_loss)]
            let d = delta as usize % len;
            (pos + d) % len
        } else {
            #[allow(clippy::cast_sign_loss)]
            let d = (-delta) as usize % len;
            (pos + len - d) % len
        };
        Self::ALL[stepped]
    }
}

/// Ephemeral on-screen overlay displaying the most recent keystroke.
/// Surfaces in the corner of the body area for [`KEY_OVERLAY_TTL`] so a
/// viewer (especially of a recorded demo) can tell what was pressed.
#[derive(Debug, Clone)]
pub struct KeyOverlay {
    pub label: String,
    pub expires_at: Instant,
}

/// UI state for `/top`. Pure data — no I/O, no side effects.
///
/// The `#[allow]` covers the half-dozen independent UI flags (`extended`,
/// `sort_desc`, `force_refresh`, `should_exit`, …). They are orthogonal
/// one-shot signals, not states of a state machine, so collapsing them
/// into an enum would cost clarity for no benefit.
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)]
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
    /// Currently active sort column; `<` / `>` cycle, `r` toggles.
    pub sort_column: SortColumn,
    /// `true` when sorting descending (largest first); toggled by `r`.
    pub sort_desc: bool,
    /// When `true`, every keystroke seeds [`App::last_key`] so the
    /// renderer can draw a temporary key-press overlay. Off by default —
    /// it's a recording aid (used by `demos/top-demo.tape` via
    /// `--show-keys`), not something an interactive user wants flashing
    /// in their face.
    pub show_keys: bool,
    /// Most recent keystroke and its overlay expiration time. Populated
    /// only when `show_keys` is on.
    pub last_key: Option<KeyOverlay>,
    /// Set by `Space` to ask the event loop to break out of its poll
    /// window and run a sampler tick immediately (matches `top`'s
    /// space-bar behaviour). Cleared by the loop after the forced tick.
    pub force_refresh: bool,
    /// Pending `k`/`K` confirmation. While `Some`, the footer shows a
    /// y/n prompt instead of the default hint line.
    pub kill_confirm: Option<KillRequest>,
    /// Approved kill that the event loop will execute on its next pass.
    /// Cleared after the SQL is dispatched. Separate from `kill_confirm`
    /// so `handle_key` (sync, no I/O) can hand the action off to the loop.
    pub kill_pending: Option<KillRequest>,
    /// Last admin-action result, surfaced briefly in the footer until
    /// the next sampler tick (or 5 s, whichever is sooner).
    pub admin_message: Option<AdminMessage>,
}

/// Result of a kill action, surfaced in the footer.
#[derive(Debug, Clone)]
pub struct AdminMessage {
    pub text: String,
    pub level: AdminMessageLevel,
    pub expires_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminMessageLevel {
    Ok,
    Err,
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
            sort_column: SortColumn::Qtime,
            sort_desc: true,
            show_keys: false,
            last_key: None,
            force_refresh: false,
            kill_confirm: None,
            kill_pending: None,
            admin_message: None,
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
        // Record the keystroke for the corner overlay before any branch
        // returns. Off unless --show-keys is set (recording aid only).
        // Suppressed inside the prompt so each typed digit does not
        // flash in the overlay — the prompt buffer is the indicator.
        if self.show_keys && self.prompt.is_none() {
            self.note_key(&key);
        }

        // Active prompt swallows almost every key.
        if self.prompt.is_some() {
            self.handle_prompt_key(key);
            return self.should_exit;
        }

        // Kill confirmation: only y / Y fire, anything else cancels.
        if self.kill_confirm.is_some() {
            self.handle_kill_confirm_key(key);
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
            KeyCode::Up => self.cursor_up(1),
            KeyCode::Down => self.cursor_down(1),
            KeyCode::PageUp => self.cursor_up(page_size.max(1)),
            KeyCode::PageDown => self.cursor_down(page_size.max(1)),
            KeyCode::Home => self.cursor_home(),
            KeyCode::End => self.cursor_end(),
            // Vim-style cursor; intentionally no `j` because `k` is the
            // pg_cancel_backend trigger and a Vim user would expect both
            // letters bound together. Down arrow + j-disabled is the
            // safer call.
            KeyCode::Char('s') => self.open_refresh_prompt(),
            KeyCode::Char('e') => self.extended = !self.extended,
            KeyCode::Char('<' | ',') | KeyCode::Left => self.cycle_sort(-1),
            KeyCode::Char('>' | '.') | KeyCode::Right => self.cycle_sort(1),
            KeyCode::Char('r') => self.sort_desc = !self.sort_desc,
            KeyCode::Char(' ') => self.force_refresh = true,
            KeyCode::Char('k') => self.request_kill(KillMode::Cancel),
            KeyCode::Char('K') => self.request_kill(KillMode::Terminate),
            _ => {}
        }
        self.should_exit
    }

    fn handle_kill_confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y' | 'Y') => {
                self.kill_pending = self.kill_confirm.take();
            }
            // n / N / Esc / anything else → cancel without firing.
            _ => self.kill_confirm = None,
        }
    }

    fn request_kill(&mut self, mode: KillMode) {
        let Some(snap) = self.snapshot.as_ref() else {
            return;
        };
        // The renderer sorts a fresh slice via `sort_rows` and the
        // selection cursor indexes the *sorted* view, so the kill
        // target has to come from the same sorted slice — otherwise
        // pressing `k`/`K` confirms a different pid than the one
        // highlighted (REV round-11 blocking finding).
        let mut sorted: Vec<&ActivityRow> = snap.rows.iter().collect();
        crate::top::views::activity::sort_rows(&mut sorted, self.sort_column, self.sort_desc);
        let Some(&row) = sorted.get(self.selected_row) else {
            return;
        };
        // Don't try to kill background workers / non-client backends.
        if row.pid <= 0 {
            return;
        }
        self.kill_confirm = Some(KillRequest::from_row(mode, row));
    }

    /// Cycle the active sort column by `delta` positions (left = -1,
    /// right = +1). Direction resets to the column's default each time
    /// the column itself changes; `r` toggles direction without moving.
    pub fn cycle_sort(&mut self, delta: isize) {
        let next = self.sort_column.step(delta);
        if next != self.sort_column {
            self.sort_column = next;
            self.sort_desc = next.default_desc();
        }
    }

    fn note_key(&mut self, key: &KeyEvent) {
        let label = format_key_label(key);
        if label.is_empty() {
            return;
        }
        self.last_key = Some(KeyOverlay {
            label,
            expires_at: Instant::now() + KEY_OVERLAY_TTL,
        });
    }

    /// Read the key overlay if it is still fresh. Renderers call this
    /// instead of touching `last_key` directly so the staleness check
    /// stays in one place.
    pub fn fresh_key_overlay(&self) -> Option<&KeyOverlay> {
        self.last_key
            .as_ref()
            .filter(|ko| Instant::now() < ko.expires_at)
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

/// Default `--ts-format` for `--batch` output. Plain ISO 8601 UTC,
/// chosen to be unambiguous for log files and grep-friendly.
pub const DEFAULT_TS_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";

/// Calendar date / time-of-day broken out of a Unix timestamp (UTC).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalDate {
    pub year: i64,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// Convert Unix `secs` (UTC) into a calendar date. Uses Howard Hinnant's
/// `civil_from_days` algorithm so we avoid pulling chrono in for `--batch`
/// timestamp formatting.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn cal_date_utc(unix_secs: i64) -> CalDate {
    let day_secs = unix_secs.rem_euclid(86_400);
    let days = unix_secs.div_euclid(86_400);

    let hour = (day_secs / 3600) as u8;
    let minute = ((day_secs % 3600) / 60) as u8;
    let second = (day_secs % 60) as u8;

    // Howard Hinnant — http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719_468;
    let era = if z >= 0 {
        z / 146_097
    } else {
        (z - 146_096) / 146_097
    };
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u8;
    if month <= 2 {
        year += 1;
    }

    CalDate {
        year,
        month,
        day,
        hour,
        minute,
        second,
    }
}

/// Format a Unix timestamp using a tiny strftime subset:
///
/// | Token | Expansion              |
/// |-------|------------------------|
/// | `%Y`  | 4-digit year           |
/// | `%m`  | 2-digit month          |
/// | `%d`  | 2-digit day            |
/// | `%H`  | 2-digit hour (24h)     |
/// | `%M`  | 2-digit minute         |
/// | `%S`  | 2-digit second         |
/// | `%T`  | `HH:MM:SS`             |
/// | `%F`  | `YYYY-MM-DD`           |
/// | `%s`  | unix seconds since epoch |
/// | `%z`  | `+0000` (always UTC)   |
/// | `%Z`  | `UTC`                  |
/// | `%%`  | literal `%`            |
///
/// Unknown specifiers pass through verbatim (`%X` → `%X`) so a typo is
/// visible in the output rather than silently swallowed.
pub fn format_strftime(fmt: &str, unix_secs: i64) -> String {
    use std::fmt::Write;

    let cal = cal_date_utc(unix_secs);
    let mut out = String::with_capacity(fmt.len());
    let mut chars = fmt.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('Y') => write!(out, "{:04}", cal.year).expect("write to String"),
            Some('m') => write!(out, "{:02}", cal.month).expect("write to String"),
            Some('d') => write!(out, "{:02}", cal.day).expect("write to String"),
            Some('H') => write!(out, "{:02}", cal.hour).expect("write to String"),
            Some('M') => write!(out, "{:02}", cal.minute).expect("write to String"),
            Some('S') => write!(out, "{:02}", cal.second).expect("write to String"),
            Some('T') => write!(out, "{:02}:{:02}:{:02}", cal.hour, cal.minute, cal.second)
                .expect("write to String"),
            Some('F') => write!(out, "{:04}-{:02}-{:02}", cal.year, cal.month, cal.day)
                .expect("write to String"),
            Some('s') => write!(out, "{unix_secs}").expect("write to String"),
            Some('z') => out.push_str("+0000"),
            Some('Z') => out.push_str("UTC"),
            // A literal `%%` and a trailing bare `%` both render as one `%`.
            Some('%') | None => out.push('%'),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
        }
    }
    out
}

/// Compact human label for a key press, used by the corner overlay.
///
/// Modifier handling:
///   - `Shift` is mostly implicit: typing 'K' already arrives as
///     `Char('K')` with the SHIFT modifier set, so we don't add a
///     visible "⇧" prefix for the common case. Exception: `BackTab`
///     (Shift-Tab) gets an explicit "⇧Tab" label since the key code
///     itself encodes the modifier.
///   - `Ctrl` / `Alt` / `Super` (Meta on macOS / Windows key) are
///     prepended in `top`-style abbreviations: `C-`, `M-`, `S-`. They
///     stack: `Ctrl-Alt-X` renders as `C-M-X`.
///   - Crossterm exposes the modifier on most terminals; macOS
///     terminals routinely strip Alt/Super, so those badges won't
///     fire there — that's a terminal limitation, not ours.
pub fn format_key_label(key: &KeyEvent) -> String {
    let mods = key.modifiers;
    let mut prefix = String::new();
    if mods.contains(KeyModifiers::CONTROL) {
        prefix.push_str("C-");
    }
    if mods.contains(KeyModifiers::ALT) {
        prefix.push_str("M-");
    }
    if mods.contains(KeyModifiers::SUPER) {
        prefix.push_str("S-");
    }

    let body = match key.code {
        KeyCode::Up => "↑".to_owned(),
        KeyCode::Down => "↓".to_owned(),
        KeyCode::Left => "←".to_owned(),
        KeyCode::Right => "→".to_owned(),
        KeyCode::PageUp => "PgUp".to_owned(),
        KeyCode::PageDown => "PgDn".to_owned(),
        KeyCode::Home => "Home".to_owned(),
        KeyCode::End => "End".to_owned(),
        KeyCode::Tab => "Tab".to_owned(),
        KeyCode::BackTab => "⇧Tab".to_owned(),
        KeyCode::Enter => "↩".to_owned(),
        KeyCode::Esc => "Esc".to_owned(),
        KeyCode::Backspace => "⌫".to_owned(),
        KeyCode::Delete => "⌦".to_owned(),
        KeyCode::Char(c) => c.to_string(),
        _ => return String::new(),
    };
    format!("{prefix}{body}")
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
    fn k_no_longer_aliases_cursor_up() {
        // `k` is now the cancel-backend trigger (matches top), no longer
        // a vim-style cursor movement. Make sure the binding swap really
        // happened so future refactors don't quietly bring vim-`k` back
        // and then surprise an operator with an accidental cancellation.
        let mut app = App::new();
        app.set_snapshot(snap_with(3));
        app.handle_key(key(KeyCode::Down), 10);
        app.handle_key(key(KeyCode::Down), 10);
        assert_eq!(app.selected_row, 2);
        app.handle_key(key(KeyCode::Char('k')), 10);
        // Cursor unchanged; `k` opened a confirm prompt instead.
        assert_eq!(app.selected_row, 2);
        assert!(app.kill_confirm.is_some());
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

    // -- Sort cycling ---------------------------------------------------------

    #[test]
    fn default_sort_is_qtime_descending() {
        let app = App::new();
        assert_eq!(app.sort_column, SortColumn::Qtime);
        assert!(app.sort_desc);
    }

    #[test]
    fn gt_advances_sort_column_lt_rewinds() {
        let mut app = App::new();
        // Default = Qtime; > goes to Xtime.
        app.handle_key(key(KeyCode::Char('>')), 10);
        assert_eq!(app.sort_column, SortColumn::Xtime);
        app.handle_key(key(KeyCode::Char('>')), 10);
        assert_eq!(app.sort_column, SortColumn::Locks);
        app.handle_key(key(KeyCode::Char('<')), 10);
        assert_eq!(app.sort_column, SortColumn::Xtime);
    }

    #[test]
    fn sort_cycler_wraps_around() {
        let mut app = App::new();
        // From Qtime, two `<` lands on Wait then State (relative).
        // From Pid, one `<` wraps to Query (the last column).
        app.sort_column = SortColumn::Pid;
        app.handle_key(key(KeyCode::Char('<')), 10);
        assert_eq!(app.sort_column, SortColumn::Query);
        app.handle_key(key(KeyCode::Char('>')), 10);
        assert_eq!(app.sort_column, SortColumn::Pid);
    }

    #[test]
    fn r_toggles_direction_only() {
        let mut app = App::new();
        let col = app.sort_column;
        let was_desc = app.sort_desc;
        app.handle_key(key(KeyCode::Char('r')), 10);
        assert_eq!(app.sort_column, col);
        assert_eq!(app.sort_desc, !was_desc);
    }

    // -- Kill confirmation ---------------------------------------------------

    fn snap_with_pid(pid: i32, query: &str) -> Snapshot {
        Snapshot {
            ts: 1,
            server: ServerSummary::default(),
            rows: vec![ActivityRow {
                pid,
                usename: "nik".into(),
                datname: "prod".into(),
                state: "active".into(),
                query: query.into(),
                ..Default::default()
            }],
        }
    }

    #[test]
    fn k_opens_cancel_confirmation_with_selected_row_data() {
        let mut app = App::new();
        app.set_snapshot(snap_with_pid(1234, "select pg_sleep(60)"));
        app.handle_key(key(KeyCode::Char('k')), 10);
        let req = app.kill_confirm.as_ref().expect("k opens confirm");
        assert_eq!(req.mode, KillMode::Cancel);
        assert_eq!(req.pid, 1234);
        assert_eq!(req.usename, "nik");
        assert!(req.query_summary.starts_with("select pg_sleep"));
    }

    #[test]
    fn shift_k_opens_terminate_confirmation() {
        let mut app = App::new();
        app.set_snapshot(snap_with_pid(1234, "select 1"));
        app.handle_key(key(KeyCode::Char('K')), 10);
        let req = app.kill_confirm.as_ref().expect("K opens confirm");
        assert_eq!(req.mode, KillMode::Terminate);
    }

    #[test]
    fn k_is_a_no_op_with_no_rows() {
        let mut app = App::new();
        app.handle_key(key(KeyCode::Char('k')), 10);
        assert!(app.kill_confirm.is_none());
    }

    #[test]
    fn k_is_a_no_op_for_zero_pid_rows() {
        // Background workers (checkpointer, autovacuum launcher, …) come
        // back from pg_stat_activity with pid 0 in our fixture model. The
        // confirm prompt must reject them rather than firing pg_cancel
        // on pid 0.
        let mut app = App::new();
        let mut snap = snap_with_pid(0, "(no query)");
        snap.rows[0].state.clear();
        app.set_snapshot(snap);
        app.handle_key(key(KeyCode::Char('k')), 10);
        assert!(app.kill_confirm.is_none());
    }

    #[test]
    fn k_is_a_no_op_for_negative_pid_rows() {
        // Walsender slots can surface with pid < 0. Same guard applies.
        let mut app = App::new();
        app.set_snapshot(snap_with_pid(-1, "(no query)"));
        app.handle_key(key(KeyCode::Char('k')), 10);
        assert!(app.kill_confirm.is_none());
    }

    #[test]
    fn y_promotes_confirm_to_pending() {
        let mut app = App::new();
        app.set_snapshot(snap_with_pid(1234, "select 1"));
        app.handle_key(key(KeyCode::Char('k')), 10);
        assert!(app.kill_confirm.is_some());

        app.handle_key(key(KeyCode::Char('y')), 10);
        assert!(app.kill_confirm.is_none(), "confirm consumed");
        let pending = app.kill_pending.as_ref().expect("pending set");
        assert_eq!(pending.pid, 1234);
        assert_eq!(pending.mode, KillMode::Cancel);
    }

    #[test]
    fn n_and_esc_cancel_kill_confirmation_without_firing() {
        for cancel_key in [KeyCode::Char('n'), KeyCode::Char('N'), KeyCode::Esc] {
            let mut app = App::new();
            app.set_snapshot(snap_with_pid(1234, "select 1"));
            app.handle_key(key(KeyCode::Char('K')), 10);
            assert!(app.kill_confirm.is_some());
            app.handle_key(key(cancel_key), 10);
            assert!(app.kill_confirm.is_none());
            assert!(app.kill_pending.is_none());
            assert!(
                !app.should_exit,
                "Esc must not exit while kill prompt is open"
            );
        }
    }

    #[test]
    fn k_targets_the_sorted_row_not_the_sql_row() {
        // Two rows that disagree on SQL order vs `SortColumn::Pid`-asc
        // order. The renderer sorts the slice in `views::activity` and
        // the cursor indexes the *sorted* slice. `request_kill` must
        // do the same — otherwise pressing `k` on the highlighted row
        // confirms a kill against a different backend.
        let mut app = App::new();
        let snap = Snapshot {
            ts: 1,
            server: ServerSummary::default(),
            rows: vec![
                ActivityRow {
                    pid: 7777,
                    usename: "nik".into(),
                    datname: "prod".into(),
                    state: "active".into(),
                    query: "select pg_sleep(60)".into(),
                    ..Default::default()
                },
                ActivityRow {
                    pid: 1111,
                    usename: "nik".into(),
                    datname: "prod".into(),
                    state: "active".into(),
                    query: "select 1".into(),
                    ..Default::default()
                },
            ],
        };
        app.set_snapshot(snap);
        // Sort by Pid ascending. Sorted slice is now [1111, 7777];
        // SQL slice is still [7777, 1111].
        app.sort_column = SortColumn::Pid;
        app.sort_desc = false;
        // Cursor on the second row of the sorted slice → pid 7777.
        app.selected_row = 1;
        app.handle_key(key(KeyCode::Char('k')), 10);
        let req = app
            .kill_confirm
            .as_ref()
            .expect("k must open the confirm prompt");
        assert_eq!(
            req.pid, 7777,
            "kill confirm must target the sorted-slice row at selected_row, \
             not snap.rows[selected_row]"
        );
    }

    #[test]
    fn k_targets_sorted_row_under_qtime_sort() {
        // Same invariant as k_targets_the_sorted_row_not_the_sql_row but
        // under Qtime-descending. Row with higher qtime must be targeted.
        let mut app = App::new();
        let snap = Snapshot {
            ts: 1,
            server: ServerSummary::default(),
            rows: vec![
                ActivityRow {
                    pid: 100,
                    usename: "a".into(),
                    datname: "d".into(),
                    state: "active".into(),
                    qtime_secs: Some(1.0),
                    ..Default::default()
                },
                ActivityRow {
                    pid: 999,
                    usename: "b".into(),
                    datname: "d".into(),
                    state: "active".into(),
                    qtime_secs: Some(60.0),
                    ..Default::default()
                },
            ],
        };
        app.set_snapshot(snap);
        app.sort_column = SortColumn::Qtime;
        app.sort_desc = true; // sorted: [999 (60s), 100 (1s)]
        app.selected_row = 0; // cursor on the 60s row → pid 999
        app.handle_key(key(KeyCode::Char('k')), 10);
        let req = app.kill_confirm.as_ref().expect("k must open confirm");
        assert_eq!(req.pid, 999, "cursor must target Qtime-sorted row");
    }

    #[test]
    fn page_down_saturates_at_last_row() {
        // PageDown with page_size > row_count must clamp at the last row,
        // not wrap or panic.
        let mut app = App::new();
        app.set_snapshot(snap_with(3)); // 3 rows
        app.handle_key(key(KeyCode::PageDown), 10); // page_size 10 > 3 rows
        assert_eq!(
            app.selected_row, 2,
            "PageDown past end must saturate at last row"
        );
    }

    #[test]
    fn sort_right_edge_wraps_from_query_to_pid() {
        let mut app = App::new();
        app.sort_column = SortColumn::Query; // last column
        app.handle_key(key(KeyCode::Char('>')), 10);
        assert_eq!(
            app.sort_column,
            SortColumn::Pid,
            "> from last column must wrap to first"
        );
    }

    #[test]
    fn set_snapshot_clears_last_error() {
        let mut app = App::new();
        app.note_error("connection lost".into());
        assert!(app.last_error.is_some());
        app.set_snapshot(snap_with(1));
        assert!(
            app.last_error.is_none(),
            "set_snapshot must clear last_error"
        );
    }

    #[test]
    fn left_right_arrows_cycle_sort_column() {
        let mut app = App::new();
        let start = app.sort_column;
        app.handle_key(key(KeyCode::Right), 10);
        assert_ne!(app.sort_column, start);
        let after_right = app.sort_column;
        app.handle_key(key(KeyCode::Left), 10);
        assert_eq!(app.sort_column, start, "Left should rewind Right");
        app.handle_key(key(KeyCode::Left), 10);
        assert_ne!(app.sort_column, after_right);
    }

    #[test]
    fn space_sets_force_refresh_flag() {
        let mut app = App::new();
        assert!(!app.force_refresh);
        app.handle_key(key(KeyCode::Char(' ')), 10);
        assert!(app.force_refresh);
    }

    #[test]
    fn changing_column_resets_to_default_direction() {
        let mut app = App::new();
        // Start at Qtime desc, reverse via r → Qtime asc.
        app.handle_key(key(KeyCode::Char('r')), 10);
        assert!(!app.sort_desc);
        // Cycle to Xtime — desc by default.
        app.handle_key(key(KeyCode::Char('>')), 10);
        assert_eq!(app.sort_column, SortColumn::Xtime);
        assert!(app.sort_desc);
        // Cycle into User (textual default → asc).
        for _ in 0..5 {
            app.handle_key(key(KeyCode::Char('<')), 10);
        }
        assert_eq!(app.sort_column, SortColumn::User);
        assert!(!app.sort_desc);
    }

    // -- Key overlay ----------------------------------------------------------

    #[test]
    fn pressing_a_key_seeds_the_overlay_with_its_label() {
        let mut app = App::new();
        app.show_keys = true;
        app.handle_key(key(KeyCode::Down), 10);
        let ko = app
            .fresh_key_overlay()
            .expect("overlay should be set after a keypress");
        assert_eq!(ko.label, "↓");

        app.handle_key(key(KeyCode::Char('e')), 10);
        assert_eq!(app.fresh_key_overlay().unwrap().label, "e");

        // Comma is mapped to "<" only when shifted; the bare "," still
        // acts as a sort-cycler but renders as "," in the overlay.
        app.handle_key(key(KeyCode::Char('<')), 10);
        assert_eq!(app.fresh_key_overlay().unwrap().label, "<");
    }

    #[test]
    fn key_overlay_is_suppressed_inside_prompt() {
        let mut app = App::new();
        app.show_keys = true;
        app.open_refresh_prompt();
        // open_refresh_prompt itself does not touch the overlay; subsequent
        // typed chars should not surface in the corner.
        app.handle_key(key(KeyCode::Char('5')), 10);
        assert!(app.fresh_key_overlay().is_none());
    }

    #[test]
    fn modifier_keys_get_compact_prefixes() {
        // C- (Ctrl), M- (Alt / Meta), S- (Super) — top-style.
        let make = |code: KeyCode, mods: KeyModifiers| KeyEvent::new(code, mods);

        assert_eq!(
            format_key_label(&make(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            "C-c"
        );
        assert_eq!(
            format_key_label(&make(KeyCode::Char('x'), KeyModifiers::ALT)),
            "M-x"
        );
        assert_eq!(
            format_key_label(&make(
                KeyCode::Char('k'),
                KeyModifiers::CONTROL | KeyModifiers::ALT
            )),
            "C-M-k"
        );
        // Shift on a printable arrives as the uppercased char itself —
        // we don't add an explicit shift prefix in that case.
        assert_eq!(
            format_key_label(&make(KeyCode::Char('K'), KeyModifiers::SHIFT)),
            "K"
        );
        // BackTab is the canonical Shift-Tab and gets an explicit ⇧Tab.
        assert_eq!(
            format_key_label(&make(KeyCode::BackTab, KeyModifiers::SHIFT)),
            "⇧Tab"
        );
    }

    #[test]
    fn key_overlay_off_by_default() {
        let mut app = App::new();
        // No show_keys = no overlay even after a key press.
        app.handle_key(key(KeyCode::Char('e')), 10);
        assert!(
            app.fresh_key_overlay().is_none(),
            "key overlay must default to off"
        );
    }

    // -- strftime helper -----------------------------------------------------

    #[test]
    fn cal_date_utc_anchor_points() {
        // 1970-01-01T00:00:00Z
        let z = cal_date_utc(0);
        assert_eq!(
            (z.year, z.month, z.day, z.hour, z.minute, z.second),
            (1970, 1, 1, 0, 0, 0)
        );
        // 2023-11-14T22:13:20Z (1_700_000_000)
        let n = cal_date_utc(1_700_000_000);
        assert_eq!(
            (n.year, n.month, n.day, n.hour, n.minute, n.second),
            (2023, 11, 14, 22, 13, 20)
        );
        // Leap-year guard: 2020-02-29
        let leap = cal_date_utc(1_582_934_400);
        assert_eq!((leap.year, leap.month, leap.day), (2020, 2, 29));
    }

    #[test]
    fn format_strftime_default_iso8601() {
        let s = format_strftime(DEFAULT_TS_FORMAT, 1_700_000_000);
        assert_eq!(s, "2023-11-14T22:13:20Z");
    }

    #[test]
    fn format_strftime_subset_tokens() {
        let t = 1_700_000_000;
        assert_eq!(format_strftime("%F %T", t), "2023-11-14 22:13:20");
        assert_eq!(format_strftime("%H:%M:%S", t), "22:13:20");
        assert_eq!(format_strftime("%Y%m%d-%H%M%S", t), "20231114-221320");
        assert_eq!(format_strftime("%s", t), "1700000000");
        assert_eq!(format_strftime("%z %Z", t), "+0000 UTC");
        assert_eq!(format_strftime("100%%", t), "100%");
    }

    #[test]
    fn format_strftime_unknown_token_passes_through() {
        assert_eq!(
            format_strftime("[%Q]", 0),
            "[%Q]",
            "unknown specifier should not be swallowed"
        );
        // A trailing bare % survives as a literal (no panic).
        assert_eq!(format_strftime("ends with %", 0), "ends with %");
    }

    #[test]
    fn fresh_key_overlay_expires_after_ttl() {
        let mut app = App::new();
        app.show_keys = true;
        app.handle_key(key(KeyCode::Char('e')), 10);
        assert!(app.fresh_key_overlay().is_some());

        // Stamp it as already-expired by hand — we cannot freeze time, so
        // the test exercises the staleness check, not the wall-clock TTL.
        let ko = app.last_key.as_mut().unwrap();
        ko.expires_at = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_millis(1))
            .expect("clock has advanced past 1 ms since boot");
        assert!(app.fresh_key_overlay().is_none());
    }
}
