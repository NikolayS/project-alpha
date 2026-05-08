//! Top-level draw routine for `/top`. Composes the header bar, tabs strip,
//! body (delegated to a per-view renderer), and footer hint line.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};
use ratatui::Frame;

use super::state::{App, Snapshot, View};
use super::theme::Theme;
use super::views::activity;

/// Min terminal size below which we render a "too small" stub instead of the
/// real UI. 24 rows × 80 cols matches the project-wide minimum used by
/// `/ash` (`src/ash/renderer.rs`).
const MIN_ROWS: u16 = 24;
const MIN_COLS: u16 = 80;

/// Top-level entry. Pulls all rendering parameters off `App`.
pub fn draw(frame: &mut Frame, app: &App, theme: &Theme) {
    let area = frame.area();
    if area.width < MIN_COLS || area.height < MIN_ROWS {
        render_too_small(frame, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Length(1), // tabs
            Constraint::Min(3),    // body
            Constraint::Length(1), // footer
        ])
        .split(area);

    render_header(frame, chunks[0], app, theme);
    render_tabs(frame, chunks[1], app, theme);
    render_body(frame, chunks[2], app, theme);
    render_footer(frame, chunks[3], app, theme);
}

// ---------------------------------------------------------------------------
// Header
// ---------------------------------------------------------------------------

fn render_header(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme.border)
        .title(Line::from(Span::styled(" rpg /top ", theme.title)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(inner);

    let snap = app.snapshot.as_ref();
    let summary_line = build_summary_line(snap, theme);
    let counts_line = build_counts_line(snap, app, theme);

    frame.render_widget(Paragraph::new(summary_line), columns[0]);
    frame.render_widget(
        Paragraph::new(counts_line).alignment(ratatui::layout::Alignment::Right),
        columns[1],
    );
}

fn build_summary_line<'a>(snap: Option<&'a Snapshot>, theme: &'a Theme) -> Line<'a> {
    if let Some(s) = snap {
        let recovery = if s.server.in_recovery {
            "standby"
        } else {
            "primary"
        };
        Line::from(vec![
            Span::styled("db ", theme.muted),
            Span::raw(s.server.db_name.clone()),
            Span::styled("  user ", theme.muted),
            Span::raw(s.server.user.clone()),
            Span::styled("  pg ", theme.muted),
            Span::raw(s.server.pg_version.clone()),
            Span::styled("  ", theme.muted),
            Span::raw(recovery),
            Span::styled("  uptime ", theme.muted),
            Span::raw(format_uptime(s.server.uptime_secs)),
            Span::styled("  as of T", theme.muted),
            Span::raw(s.ts.to_string()),
        ])
    } else {
        Line::from(Span::styled("connecting…", theme.muted))
    }
}

fn build_counts_line<'a>(snap: Option<&'a Snapshot>, app: &'a App, theme: &'a Theme) -> Line<'a> {
    let connection_dot = if app.stale_ticks == 0 && snap.is_some() {
        Span::styled("● ", theme.status_ok)
    } else {
        Span::styled("● ", theme.status_stale)
    };

    let mut spans = vec![connection_dot];
    if let Some(s) = snap {
        spans.extend([
            Span::styled("active ", theme.muted),
            Span::raw(s.server.active.to_string()),
            Span::styled("  idle-in-tx ", theme.muted),
            Span::raw(s.server.idle_in_tx.to_string()),
            Span::styled("  wait ", theme.muted),
            Span::raw(s.server.waiting.to_string()),
            Span::styled("  total ", theme.muted),
            Span::raw(s.server.total_backends.to_string()),
        ]);
        if app.stale_ticks > 0 {
            spans.push(Span::styled(
                format!("  stale {}", app.stale_ticks),
                theme.status_stale,
            ));
        }
    } else {
        spans.push(Span::styled(
            "active –  idle-in-tx –  wait –  total –",
            theme.muted,
        ));
    }
    Line::from(spans)
}

fn format_uptime(secs: i64) -> String {
    let s = secs.max(0);
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86_400 {
        format!("{}h{:02}m", s / 3600, (s % 3600) / 60)
    } else {
        format!("{}d{:02}h", s / 86_400, (s % 86_400) / 3600)
    }
}

// ---------------------------------------------------------------------------
// Tabs
// ---------------------------------------------------------------------------

fn render_tabs(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    // S1: only the Activity tab. The data structure is here so S2 can drop
    // in `Databases`, `Tables`, … without touching the renderer entry point.
    let titles = vec![Line::from(Span::styled(
        View::Activity.label(),
        theme.title,
    ))];
    let selected = match app.view {
        View::Activity => 0,
    };
    let tabs = Tabs::new(titles)
        .select(selected)
        .style(theme.muted)
        .highlight_style(
            theme
                .title
                .add_modifier(Modifier::REVERSED)
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::styled(" │ ", theme.muted));
    frame.render_widget(tabs, area);
}

// ---------------------------------------------------------------------------
// Body
// ---------------------------------------------------------------------------

fn render_body(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    match app.view {
        View::Activity => activity::render(frame, area, app, theme),
    }
}

// ---------------------------------------------------------------------------
// Footer
// ---------------------------------------------------------------------------

fn render_footer(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let line = if let Some(err) = app.last_error.as_deref() {
        Line::from(vec![
            Span::styled(" error ", Style::default().fg(ratatui::style::Color::Red)),
            Span::raw(truncate_err(err, area.width.saturating_sub(8) as usize)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" q ", theme.title),
            Span::styled("quit  ", theme.footer),
            Span::styled("↑↓ ", theme.title),
            Span::styled("move  ", theme.footer),
            Span::styled("Home/End ", theme.title),
            Span::styled("first/last  ", theme.footer),
            Span::styled("PgUp/PgDn ", theme.title),
            Span::styled("page  ", theme.footer),
        ])
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn truncate_err(s: &str, max: usize) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    if cleaned.chars().count() <= max {
        cleaned
    } else {
        let mut out: String = cleaned.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

// ---------------------------------------------------------------------------
// Too-small stub
// ---------------------------------------------------------------------------

fn render_too_small(frame: &mut Frame, area: Rect) {
    let p = Paragraph::new(format!(
        "rpg /top requires a terminal of at least {MIN_COLS}×{MIN_ROWS}.\nResize and try again."
    ))
    .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(p, area);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::top::state::{ActivityRow, ServerSummary};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn fixture_snapshot() -> Snapshot {
        Snapshot {
            ts: 1_700_000_000,
            server: ServerSummary {
                db_name: "prod".into(),
                user: "nik".into(),
                pg_version: "16.4".into(),
                uptime_secs: 14 * 86_400 + 3 * 3600,
                in_recovery: false,
                active: 17,
                idle_in_tx: 3,
                waiting: 2,
                total_backends: 22,
            },
            rows: vec![
                ActivityRow {
                    pid: 12_345,
                    usename: "app".into(),
                    datname: "prod".into(),
                    application_name: "web-1".into(),
                    client_addr: "10.0.0.5".into(),
                    backend_type: "client backend".into(),
                    state: "active".into(),
                    wait_event_type: "IO".into(),
                    wait_event: "DataFileRead".into(),
                    qtime_secs: Some(42.0),
                    xtime_secs: Some(42.0),
                    query: "update accounts set balance = balance + 1 where id = 5".into(),
                },
                ActivityRow {
                    pid: 12_346,
                    usename: "etl".into(),
                    datname: "analytics".into(),
                    application_name: "etl-runner".into(),
                    client_addr: "10.0.0.7".into(),
                    backend_type: "client backend".into(),
                    state: "active".into(),
                    wait_event_type: "Lock".into(),
                    wait_event: "transactionid".into(),
                    qtime_secs: Some(2.3),
                    xtime_secs: Some(1020.0),
                    query: "select count(*) from events where ts > now() - interval '1 day'".into(),
                },
                ActivityRow {
                    pid: 12_350,
                    usename: "nik".into(),
                    datname: "prod".into(),
                    application_name: "psql".into(),
                    client_addr: String::new(),
                    backend_type: "client backend".into(),
                    state: "idle in transaction".into(),
                    wait_event_type: "Client".into(),
                    wait_event: "ClientRead".into(),
                    qtime_secs: Some(5.0),
                    xtime_secs: Some(125.0),
                    query: "begin".into(),
                },
            ],
        }
    }

    fn render_into(width: u16, height: u16, app: &App) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut term = Terminal::new(backend).expect("create test terminal");
        let theme = Theme::for_tests();
        term.draw(|f| draw(f, app, &theme)).expect("draw");
        term.backend().buffer().clone()
    }

    #[test]
    fn renders_loading_when_no_snapshot() {
        let app = App::new();
        let buf = render_into(120, 30, &app);
        let dump = buffer_to_string(&buf);
        assert!(dump.contains("rpg /top"), "header missing");
        assert!(dump.contains("Activity"), "tabs missing");
        assert!(
            dump.contains("collecting first sample"),
            "loading hint missing"
        );
        assert!(dump.contains("quit"), "footer hint missing");
    }

    #[test]
    fn renders_activity_rows_with_summary() {
        let mut app = App::new();
        app.set_snapshot(fixture_snapshot());
        let buf = render_into(120, 30, &app);
        let dump = buffer_to_string(&buf);
        assert!(dump.contains("db prod"), "summary db missing");
        assert!(dump.contains("active 17"), "active count missing");
        assert!(dump.contains("idle-in-tx 3"), "idle-in-tx count missing");
        assert!(dump.contains("12345"), "first pid missing");
        assert!(dump.contains("update accounts"), "query text missing");
        assert!(dump.contains("(3 rows)"), "row count caption missing");
        assert!(dump.contains("Activity"), "tab label missing");
    }

    #[test]
    fn renders_empty_state_when_zero_rows() {
        let mut app = App::new();
        app.set_snapshot(Snapshot {
            ts: 1,
            server: ServerSummary {
                db_name: "prod".into(),
                user: "nik".into(),
                pg_version: "16".into(),
                ..Default::default()
            },
            rows: vec![],
        });
        let buf = render_into(120, 30, &app);
        let dump = buffer_to_string(&buf);
        assert!(dump.contains("no active backends"), "empty hint missing");
    }

    #[test]
    fn renders_too_small_stub_below_min_size() {
        let mut app = App::new();
        app.set_snapshot(fixture_snapshot());
        let buf = render_into(60, 10, &app);
        let dump = buffer_to_string(&buf);
        assert!(
            dump.contains("requires a terminal of at least"),
            "too-small stub missing: {dump}"
        );
    }

    #[test]
    fn narrow_layout_omits_app_and_client_columns() {
        let mut app = App::new();
        app.set_snapshot(fixture_snapshot());
        let buf = render_into(80, 30, &app);
        let dump = buffer_to_string(&buf);
        assert!(dump.contains("12345"), "narrow layout dropped pid");
        assert!(
            !dump.contains("10.0.0.5"),
            "narrow layout should drop client column"
        );
    }

    #[test]
    fn footer_shows_error_when_set() {
        let mut app = App::new();
        app.set_snapshot(fixture_snapshot());
        app.note_error("connection lost".into());
        let buf = render_into(120, 30, &app);
        let dump = buffer_to_string(&buf);
        assert!(dump.contains("error"), "error label missing");
        assert!(dump.contains("connection lost"), "error message missing");
    }

    #[test]
    fn header_shows_stale_badge_when_ticks_missed() {
        let mut app = App::new();
        app.set_snapshot(fixture_snapshot());
        app.note_stale();
        app.note_stale();
        let buf = render_into(120, 30, &app);
        let dump = buffer_to_string(&buf);
        assert!(dump.contains("stale 2"), "stale badge missing: {dump}");
    }

    fn buffer_to_string(buf: &ratatui::buffer::Buffer) -> String {
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn format_uptime_human_units() {
        assert_eq!(format_uptime(0), "0s");
        assert_eq!(format_uptime(45), "45s");
        assert_eq!(format_uptime(120), "2m");
        assert_eq!(format_uptime(3661), "1h01m");
        assert_eq!(format_uptime(90_000), "1d01h");
    }
}
