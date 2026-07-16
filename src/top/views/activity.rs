//! Activity view — one row per non-rpg backend from `pg_stat_activity`.
//!
//! Default columns: pid, user, db, state, wait, qtime, xtime, locks, query.
//! "Extended" columns (toggled by `e`): pid, user, db, **app, client,
//! backend**, state, wait, qtime, xtime, locks, query. Extended adds
//! `application_name` / `client_addr` / `backend_type` so the user can
//! spot which app or worker class is producing each row. The default
//! keeps the table readable at 80 cols; extended needs ≥120.
//!
//! Wait labels render as `Type:Event` (matching the `pg_ash` convention).
//! Wait coloring follows the `pg_ash` color scheme — see
//! [`pg_ash`](https://github.com/NikolayS/pg_ash) `docs/COLOR_SCHEME.md`.

use std::cmp::Ordering;

use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Row, Table, TableState};
use ratatui::Frame;

use crate::top::state::{ActivityRow, App, Snapshot, SortColumn};
use crate::top::theme::Theme;

/// Render the activity table into `area`. The caller is responsible for
/// passing only the body rectangle (header/tabs/footer are drawn outside).
pub fn render(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    // Body title: caption only — the view name lives in the tabs strip
    // above, no point repeating "Activity" twice. Extended-mode badge
    // still hangs off the body since it's a per-view setting.
    let mut title_spans = vec![
        Span::raw(" "),
        Span::styled(row_count_caption(app.snapshot.as_ref()), theme.muted),
    ];
    if app.extended {
        title_spans.push(Span::styled("  [extended] ", theme.title));
    }
    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(theme.border)
        .title(Line::from(title_spans));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if let Some(snap) = app.snapshot.as_ref() {
        if snap.rows.is_empty() {
            render_empty(frame, inner, theme);
        } else {
            render_table(frame, inner, snap, app, theme);
        }
    } else {
        render_loading(frame, inner, theme);
    }
}

fn row_count_caption(snap: Option<&Snapshot>) -> String {
    snap.map_or_else(
        || String::from("(loading)"),
        |s| format!("({} rows)", s.rows.len()),
    )
}

fn render_loading(frame: &mut Frame, area: Rect, theme: &Theme) {
    let p = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
        "  collecting first sample…",
        theme.muted,
    )));
    frame.render_widget(p, area);
}

fn render_empty(frame: &mut Frame, area: Rect, theme: &Theme) {
    let p = ratatui::widgets::Paragraph::new(Line::from(Span::styled(
        "  no active backends",
        theme.muted,
    )));
    frame.render_widget(p, area);
}

fn render_table(frame: &mut Frame, area: Rect, snap: &Snapshot, app: &App, theme: &Theme) {
    // Extended mode is opt-in (`e`) and requires a wide-enough terminal.
    let extended = app.extended && area.width >= 120;

    let header_cells: Vec<&str> = if extended {
        vec![
            "pid", "user", "db", "app", "client", "backend", "state", "wait", "qtime", "xtime",
            "locks", "query",
        ]
    } else {
        vec![
            "pid", "user", "db", "state", "wait", "qtime", "xtime", "locks", "query",
        ]
    };

    // Sort the rows in Rust according to the App's current sort column /
    // direction. SQL only provides the initial qtime-desc ordering; the
    // user-facing sort is applied here so that `<` / `>` / `r` take effect
    // immediately without re-sampling.
    let mut sorted: Vec<&ActivityRow> = snap.rows.iter().collect();
    sort_rows(&mut sorted, app.sort_column, app.sort_desc);

    let widths: Vec<Constraint> = if extended {
        vec![
            Constraint::Length(7),  // pid
            Constraint::Length(10), // user
            Constraint::Length(12), // db
            Constraint::Length(12), // app
            Constraint::Length(15), // client
            Constraint::Length(10), // backend_type (compact)
            Constraint::Length(8),  // state
            Constraint::Length(22), // wait Type:Event
            Constraint::Length(7),  // qtime
            Constraint::Length(7),  // xtime
            Constraint::Length(5),  // locks
            Constraint::Min(20),    // query (flex)
        ]
    } else {
        vec![
            Constraint::Length(7),  // pid
            Constraint::Length(10), // user
            Constraint::Length(12), // db
            Constraint::Length(8),  // state
            Constraint::Length(22), // wait Type:Event
            Constraint::Length(7),  // qtime
            Constraint::Length(7),  // xtime
            Constraint::Length(5),  // locks
            Constraint::Min(25),    // query (flex)
        ]
    };

    let arrow = if app.sort_desc { "▼" } else { "▲" };
    let header = Row::new(
        header_cells
            .into_iter()
            .map(|h| build_header_cell(h, app.sort_column, arrow, theme))
            .collect::<Vec<_>>(),
    )
    .height(1)
    .style(theme.header_row);

    let rows: Vec<Row> = sorted
        .iter()
        .map(|r| build_row(r, extended, theme))
        .collect();

    // Stateful Table → ratatui keeps the header sticky and auto-scrolls
    // the body so the selected row stays in view.
    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(1)
        .row_highlight_style(theme.selected);

    let mut state = TableState::default().with_selected(Some(app.selected_row));
    frame.render_stateful_widget(table, area, &mut state);
}

fn build_row<'a>(r: &'a ActivityRow, extended: bool, theme: &'a Theme) -> Row<'a> {
    let state_style = state_color(&r.state, theme);
    let wait_style = wait_color_for_row(r, theme);
    let qtime_style = qtime_warn_color(r.qtime_secs, theme);
    let wait = format_wait_label(r);

    let cells: Vec<Cell> = if extended {
        vec![
            Cell::from(r.pid.to_string()),
            Cell::from(truncate(&r.usename, 10)),
            Cell::from(truncate(&r.datname, 12)),
            Cell::from(truncate(&r.application_name, 12)),
            Cell::from(truncate(&r.client_addr, 15)),
            Cell::from(truncate(&short_backend_type(&r.backend_type), 10)),
            Cell::from(Span::styled(truncate(&r.state, 8), state_style)),
            Cell::from(Span::styled(truncate(&wait, 22), wait_style)),
            Cell::from(Span::styled(format_secs(r.qtime_secs), qtime_style)),
            Cell::from(format_secs(r.xtime_secs)),
            Cell::from(format_locks(r.locks_held)),
            Cell::from(squash_query(&r.query)),
        ]
    } else {
        vec![
            Cell::from(r.pid.to_string()),
            Cell::from(truncate(&r.usename, 10)),
            Cell::from(truncate(&r.datname, 12)),
            Cell::from(Span::styled(truncate(&r.state, 8), state_style)),
            Cell::from(Span::styled(truncate(&wait, 22), wait_style)),
            Cell::from(Span::styled(format_secs(r.qtime_secs), qtime_style)),
            Cell::from(format_secs(r.xtime_secs)),
            Cell::from(format_locks(r.locks_held)),
            Cell::from(squash_query(&r.query)),
        ]
    };

    Row::new(cells)
}

/// Header cell renderer that highlights the column corresponding to the
/// active sort. The active column shows ` <name>▼ ` (or ` ▲ ` for asc) in
/// the title style; inactive columns render in the muted header style.
fn build_header_cell<'a>(
    label: &'a str,
    sort: SortColumn,
    arrow: &'a str,
    theme: &'a Theme,
) -> Cell<'a> {
    if label == sort.header_label() {
        Cell::from(Line::from(vec![
            Span::styled(label, theme.header.add_modifier(Modifier::UNDERLINED)),
            Span::raw(arrow),
        ]))
    } else {
        Cell::from(Span::styled(label, theme.header))
    }
}

/// Sort rows in-place according to the active column / direction. Stable
/// sort is preferred so equal-key rows preserve the SQL-side ordering.
pub(in crate::top) fn sort_rows(rows: &mut [&ActivityRow], col: SortColumn, desc: bool) {
    rows.sort_by(|a, b| {
        let ord = match col {
            SortColumn::Pid => a.pid.cmp(&b.pid),
            SortColumn::User => a.usename.cmp(&b.usename),
            SortColumn::Db => a.datname.cmp(&b.datname),
            SortColumn::State => a.state.cmp(&b.state),
            SortColumn::Wait => format_wait_label(a).cmp(&format_wait_label(b)),
            SortColumn::Qtime => cmp_opt_f64(a.qtime_secs, b.qtime_secs),
            SortColumn::Xtime => cmp_opt_f64(a.xtime_secs, b.xtime_secs),
            SortColumn::Locks => a.locks_held.cmp(&b.locks_held),
            SortColumn::Query => a.query.cmp(&b.query),
        };
        if desc {
            ord.reverse()
        } else {
            ord
        }
    });
}

fn cmp_opt_f64(a: Option<f64>, b: Option<f64>) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        // Treat None as "less than" any concrete value so descending sort
        // pushes idle backends below active ones.
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}

/// Render the `wait` column label, matching `pg_ash`'s `Type:Event` format.
/// Empty when the backend is idle and not on CPU; `"CPU*"` when active with
/// no `wait_event` recorded; `Type` alone when `wait_event` is empty;
/// `Type:Event` otherwise.
fn format_wait_label(r: &ActivityRow) -> String {
    if r.wait_event_type.is_empty() {
        if r.state == "active" {
            "CPU*".to_owned()
        } else {
            String::new()
        }
    } else if r.wait_event.is_empty() {
        r.wait_event_type.clone()
    } else {
        format!("{}:{}", r.wait_event_type, r.wait_event)
    }
}

/// Strip every control character (incl. ESC, BEL, BS, DEL, the C1 range)
/// from a string before letting it reach ratatui. Any field originating
/// from `pg_stat_activity` — `application_name`, `query`, `usename`,
/// `datname`, `client_addr`, `state`, `wait_event_type`, `wait_event`,
/// `backend_type` — is settable by any connected Postgres client and could
/// otherwise embed ANSI escape sequences that the DBA's terminal would
/// execute.
///
/// Whitespace controls (`\t`, `\n`, `\r`) collapse to a single space so
/// multi-line queries still read naturally; other control bytes are
/// dropped entirely. Printable Unicode is preserved.
pub(in crate::top) fn scrub_terminal_unsafe(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c == '\t' || c == '\n' || c == '\r' {
                ' '
            } else if c.is_control() {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .filter(|c| *c != '\u{FFFD}')
        .collect()
}

pub(in crate::top) fn truncate(s: &str, max: usize) -> String {
    let s = scrub_terminal_unsafe(s);
    if s.chars().count() <= max {
        s
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn squash_query(s: &str) -> String {
    let collapsed = scrub_terminal_unsafe(s);
    collapsed.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(in crate::top) fn format_secs(v: Option<f64>) -> String {
    match v {
        None => "-".to_owned(),
        Some(s) if s < 0.0 => "-".to_owned(),
        Some(s) if s < 1.0 => format!("{:.0}ms", s * 1000.0),
        Some(s) if s < 60.0 => format!("{s:.1}s"),
        Some(s) if s < 3600.0 => format!("{:.0}m", s / 60.0),
        Some(s) if s < 86_400.0 => format!("{:.0}h", s / 3600.0),
        Some(s) => format!("{:.0}d", s / 86_400.0),
    }
}

fn format_locks(n: i64) -> String {
    if n <= 0 {
        "-".to_owned()
    } else {
        n.to_string()
    }
}

/// Compact form of `pg_stat_activity.backend_type`. Most users only care
/// whether a row is a regular client backend or a parallel/maintenance
/// helper, so we strip the trailing " backend" suffix and shorten
/// "background" to keep the column narrow.
fn short_backend_type(s: &str) -> String {
    s.replace(" backend", "").replace("background ", "bg ")
}

fn state_color(state: &str, theme: &Theme) -> Style {
    match state {
        "active" => theme.state_active,
        "idle in transaction" | "idle in transaction (aborted)" => theme.state_idle_in_tx,
        "idle" => theme.muted,
        _ => Style::default(),
    }
}

/// `pg_ash` color scheme (`COLOR_SCHEME.md`):
/// idle-in-tx → light-yellow, no `wait_event_type` on an active backend →
/// CPU\* green, then `wait_event_type` → palette below.
fn wait_color_for_row(row: &ActivityRow, theme: &Theme) -> Style {
    if row.state.starts_with("idle in transaction") {
        return theme.wait_idle_tx;
    }
    if row.wait_event_type.is_empty() {
        return if row.state == "active" {
            theme.wait_cpu
        } else {
            Style::default()
        };
    }
    match row.wait_event_type.as_str() {
        "Lock" => theme.wait_lock,
        "LWLock" => theme.wait_lwlock,
        "IO" => theme.wait_io,
        "IPC" => theme.wait_ipc,
        "Client" => theme.wait_client,
        "Timeout" => theme.wait_timeout,
        "BufferPin" => theme.wait_buffer_pin,
        "Activity" => theme.wait_activity,
        "Extension" => theme.wait_extension,
        _ => theme.wait_other,
    }
}

/// Threshold-based coloring for elapsed query time.
/// Hard-coded thresholds for S1; later sprints read from `[top.thresholds]`.
fn qtime_warn_color(secs: Option<f64>, theme: &Theme) -> Style {
    match secs {
        Some(s) if s >= 30.0 => theme.qtime_crit,
        Some(s) if s >= 1.0 => theme.qtime_warn,
        _ => Style::default(),
    }
}

/// Public helper — used by the kill-confirm modal in later sprints.
#[allow(dead_code)]
pub(in crate::top) const SELECTED_HIGHLIGHT: Style = Style::new()
    .add_modifier(Modifier::REVERSED)
    .fg(Color::White);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::top::state::ActivityRow;

    #[test]
    fn truncate_short_string_unchanged() {
        assert_eq!(truncate("abc", 10), "abc");
    }

    #[test]
    fn truncate_long_string_appends_ellipsis() {
        let out = truncate("abcdefghij", 5);
        assert_eq!(out.chars().count(), 5);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn truncate_handles_unicode_correctly() {
        let out = truncate("αβγδεζ", 4);
        assert_eq!(out.chars().count(), 4);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn format_secs_uses_human_units() {
        assert_eq!(format_secs(None), "-");
        assert_eq!(format_secs(Some(0.045)), "45ms");
        assert_eq!(format_secs(Some(2.5)), "2.5s");
        assert_eq!(format_secs(Some(125.0)), "2m");
        assert_eq!(format_secs(Some(7200.0)), "2h");
        assert_eq!(format_secs(Some(172_800.0)), "2d");
    }

    #[test]
    fn format_locks_dash_when_zero() {
        assert_eq!(format_locks(0), "-");
        assert_eq!(format_locks(1), "1");
        assert_eq!(format_locks(42), "42");
    }

    #[test]
    fn squash_query_collapses_whitespace() {
        let q = "select\n    *\nfrom\tt\n  where  x = 1";
        assert_eq!(squash_query(q), "select * from t where x = 1");
    }

    #[test]
    fn row_count_caption_handles_no_snapshot() {
        assert_eq!(row_count_caption(None), "(loading)");
        let s = Snapshot::default();
        assert_eq!(row_count_caption(Some(&s)), "(0 rows)");
    }

    /// Regression: terminal-escape injection via unsanitized fields.
    #[test]
    fn user_controllable_strings_strip_control_characters() {
        let raw = "victim\x1b[2J\x1b[31mPWNED\x07\x08\rback\nnewline\ttab";
        let scrubbed = scrub_terminal_unsafe(raw);
        for ch in ['\x1b', '\x07', '\x08', '\r', '\n', '\t'] {
            assert!(
                !scrubbed.contains(ch),
                "{ch:?} must be stripped: {scrubbed:?}"
            );
        }
        assert!(scrubbed.contains("victim"));
        assert!(scrubbed.contains("PWNED"));
    }

    #[test]
    fn squash_query_strips_ansi_escapes() {
        let q = "select 1\x1b[2J\x1b[31m -- malicious";
        let out = squash_query(q);
        assert!(!out.contains('\x1b'));
        assert!(out.contains("select 1"));
    }

    #[test]
    fn truncate_strips_ansi_escapes() {
        let out = truncate("user\x1b[2J", 10);
        assert!(!out.contains('\x1b'));
    }

    /// Wait label uses ':' not '.' as delimiter (matches `pg_ash` convention).
    #[test]
    fn wait_label_uses_colon_delimiter() {
        let row = ActivityRow {
            wait_event_type: "IO".into(),
            wait_event: "DataFileRead".into(),
            state: "active".into(),
            ..Default::default()
        };
        assert_eq!(format_wait_label(&row), "IO:DataFileRead");
    }

    #[test]
    fn wait_label_empty_when_idle_and_no_event() {
        let row = ActivityRow {
            state: "idle".into(),
            ..Default::default()
        };
        assert_eq!(format_wait_label(&row), "");
    }

    #[test]
    fn wait_label_cpu_star_when_active_and_no_event() {
        let row = ActivityRow {
            state: "active".into(),
            ..Default::default()
        };
        assert_eq!(format_wait_label(&row), "CPU*");
    }

    #[test]
    fn wait_label_type_only_when_event_blank() {
        let row = ActivityRow {
            wait_event_type: "Lock".into(),
            ..Default::default()
        };
        assert_eq!(format_wait_label(&row), "Lock");
    }

    #[test]
    fn wait_color_routes_pg_ash_palette() {
        let theme = Theme::default_theme();
        let mk = |state: &str, wtype: &str| ActivityRow {
            state: state.into(),
            wait_event_type: wtype.into(),
            ..Default::default()
        };
        // Wait-event-type → distinctive pg_ash colors. We compare style fg.
        let cases = [
            ("active", "Lock", theme.wait_lock),
            ("active", "LWLock", theme.wait_lwlock),
            ("active", "IO", theme.wait_io),
            ("active", "IPC", theme.wait_ipc),
            ("active", "Client", theme.wait_client),
            ("active", "Timeout", theme.wait_timeout),
            ("active", "BufferPin", theme.wait_buffer_pin),
            ("active", "Activity", theme.wait_activity),
            ("active", "Extension", theme.wait_extension),
            ("active", "", theme.wait_cpu),
        ];
        for (state, wtype, expected) in cases {
            let row = mk(state, wtype);
            assert_eq!(
                wait_color_for_row(&row, &theme).fg,
                expected.fg,
                "state={state} wtype={wtype}"
            );
        }

        // Idle in transaction always uses the IdleTx color.
        let r = ActivityRow {
            state: "idle in transaction".into(),
            wait_event_type: "Client".into(),
            ..Default::default()
        };
        assert_eq!(
            wait_color_for_row(&r, &theme).fg,
            theme.wait_idle_tx.fg,
            "idle-in-tx must use the IdleTx color, not the Client wait color"
        );
    }
}
