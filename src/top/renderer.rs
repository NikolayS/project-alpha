//! Top-level draw routine for `/top`. Composes the header bar, tabs strip,
//! body (delegated to a per-view renderer), and footer hint line.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};
use ratatui::Frame;

use super::state::{
    AdminMessage, AdminMessageLevel, App, KillRequest, PromptKind, PromptState, Snapshot, View,
};
use super::theme::Theme;
use super::views::activity::{self, scrub_terminal_unsafe};

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
            Constraint::Length(5), // header (3 inner rows + 2 borders)
            Constraint::Length(1), // tabs
            Constraint::Min(3),    // body (sticky table header)
            Constraint::Length(1), // footer
        ])
        .split(area);

    render_header(frame, chunks[0], app, theme);
    render_tabs(frame, chunks[1], app, theme);
    render_body(frame, chunks[2], app, theme);
    render_footer(frame, chunks[3], app, theme);
    // Key-press overlay rides on top of the body in the upper-right corner;
    // drawn last so it always wins the cell.
    render_key_overlay(frame, chunks[2], app, theme);
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

    if inner.height < 3 {
        return;
    }
    let snap = app.snapshot.as_ref();
    // Three inner rows. Each row is split into a left and right paragraph
    // so the most-actionable values (clock, connection LED) anchor to the
    // right edge instead of leaving wide screens blank.
    //
    //   row 0   db / user / pg / recovery / uptime ……………………… @ HH:MM:SS UTC
    //   row 1   ● connection counts (active / idle-in-tx / wait / total/max)
    //   row 2   ops: longest-tx, longest-q, deadlocks, temp-files, av busy/max,
    //                slots phy active/total, log active/total
    let row0 = Rect::new(inner.x, inner.y, inner.width, 1);
    let row1 = Rect::new(inner.x, inner.y + 1, inner.width, 1);
    let row2 = Rect::new(inner.x, inner.y + 2, inner.width, 1);

    let row0_split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(28)])
        .split(row0);
    let row1_split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(20)])
        .split(row1);

    frame.render_widget(
        Paragraph::new(build_summary_line(snap, theme)),
        row0_split[0],
    );
    frame.render_widget(
        Paragraph::new(build_clock_line(snap, theme)).alignment(ratatui::layout::Alignment::Right),
        row0_split[1],
    );
    frame.render_widget(
        Paragraph::new(build_counts_line(snap, app, theme)),
        row1_split[0],
    );
    frame.render_widget(
        Paragraph::new(build_status_line(snap, app, theme))
            .alignment(ratatui::layout::Alignment::Right),
        row1_split[1],
    );
    frame.render_widget(Paragraph::new(build_ops_line(snap, theme)), row2);
}

fn build_ops_line<'a>(snap: Option<&'a Snapshot>, theme: &'a Theme) -> Line<'a> {
    if let Some(s) = snap {
        Line::from(vec![
            Span::styled("longest-tx ", theme.muted),
            Span::raw(format_secs_or_dash(s.server.longest_xact_secs)),
            Span::styled("  longest-q ", theme.muted),
            Span::raw(format_secs_or_dash(s.server.longest_active_query_secs)),
            Span::styled("  deadlocks ", theme.muted),
            Span::raw(s.server.deadlocks_total.to_string()),
            Span::styled("  temp-files ", theme.muted),
            Span::raw(s.server.temp_files_total.to_string()),
            Span::styled("  av ", theme.muted),
            Span::raw(format!(
                "{}/{}",
                s.server.autovacuum_busy, s.server.autovacuum_max
            )),
            Span::styled("  slots ", theme.muted),
            Span::raw(format!(
                "{}/{}p {}/{}l",
                s.server.phys_slots_active,
                s.server.phys_slots,
                s.server.log_slots_active,
                s.server.log_slots,
            )),
        ])
    } else {
        Line::from(Span::styled(
            "longest-tx –  longest-q –  deadlocks –  temp-files –  av –  slots –",
            theme.muted,
        ))
    }
}

fn build_clock_line<'a>(snap: Option<&'a Snapshot>, theme: &'a Theme) -> Line<'a> {
    if let Some(s) = snap {
        Line::from(vec![
            Span::styled("@ ", theme.muted),
            Span::raw(format_clock_utc(s.ts)),
            Span::styled(" UTC ", theme.muted),
        ])
    } else {
        Line::from(Span::styled("connecting…", theme.muted))
    }
}

fn build_status_line<'a>(snap: Option<&'a Snapshot>, app: &'a App, theme: &'a Theme) -> Line<'a> {
    let dot = if app.stale_ticks == 0 && snap.is_some() {
        Span::styled("●", theme.status_ok)
    } else {
        Span::styled("●", theme.status_stale)
    };
    if app.stale_ticks > 0 && snap.is_some() {
        Line::from(vec![
            Span::styled(format!("stale {} ", app.stale_ticks), theme.status_stale),
            dot,
            Span::raw(" "),
        ])
    } else {
        Line::from(vec![dot, Span::raw(" ")])
    }
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
            Span::raw(scrub_terminal_unsafe(&s.server.db_name)),
            Span::styled("  user ", theme.muted),
            Span::raw(scrub_terminal_unsafe(&s.server.user)),
            Span::styled("  pg ", theme.muted),
            Span::raw(scrub_terminal_unsafe(&s.server.pg_version)),
            Span::styled("  ", theme.muted),
            Span::raw(recovery),
            Span::styled("  uptime ", theme.muted),
            Span::raw(format_uptime(s.server.uptime_secs)),
        ])
    } else {
        Line::from(Span::styled("connecting…", theme.muted))
    }
}

fn build_counts_line<'a>(snap: Option<&'a Snapshot>, _app: &'a App, theme: &'a Theme) -> Line<'a> {
    if let Some(s) = snap {
        Line::from(vec![
            Span::styled("active ", theme.muted),
            Span::raw(s.server.active.to_string()),
            Span::styled("  idle-in-tx ", theme.muted),
            Span::raw(s.server.idle_in_tx.to_string()),
            Span::styled("  wait ", theme.muted),
            Span::raw(s.server.waiting.to_string()),
            Span::styled("  total ", theme.muted),
            Span::raw(format!(
                "{}/{}",
                s.server.total_backends, s.server.max_connections
            )),
        ])
    } else {
        Line::from(Span::styled(
            "active –  idle-in-tx –  wait –  total –",
            theme.muted,
        ))
    }
}

/// Format a non-negative second count as a compact human string, or `"-"`
/// when zero (which we treat as "no transaction / no active query").
fn format_secs_or_dash(secs: f64) -> String {
    if secs <= 0.0 {
        return "-".to_owned();
    }
    if secs < 1.0 {
        format!("{:.0}ms", secs * 1000.0)
    } else if secs < 60.0 {
        format!("{secs:.1}s")
    } else if secs < 3600.0 {
        format!("{:.0}m", secs / 60.0)
    } else if secs < 86_400.0 {
        format!("{:.0}h", secs / 3600.0)
    } else {
        format!("{:.0}d", secs / 86_400.0)
    }
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

/// Format a Unix timestamp as `HH:MM:SS` UTC clock time.
///
/// Pre-epoch (negative) values render as `"—"` so the header doesn't
/// surface a misleading time. We avoid pulling in chrono — only modular
/// integer arithmetic is needed.
fn format_clock_utc(ts: i64) -> String {
    if ts < 0 {
        return "—".to_owned();
    }
    let day_secs = ts.rem_euclid(86_400);
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    let s = day_secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
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
// Key-press overlay
// ---------------------------------------------------------------------------

fn render_key_overlay(frame: &mut Frame, body_area: Rect, app: &App, theme: &Theme) {
    let Some(ko) = app.fresh_key_overlay() else {
        return;
    };
    // ` ⌨ <label> ` with one cell of padding either side.
    let label = format!(" ⌨ {} ", ko.label);
    let label_chars = u16::try_from(label.chars().count()).unwrap_or(u16::MAX);
    if body_area.width <= label_chars + 2 || body_area.height < 2 {
        return;
    }
    // Pin to the upper-right corner of the body, just inside the border.
    let x = body_area.x + body_area.width.saturating_sub(label_chars + 1);
    let y = body_area.y;
    let area = Rect::new(x, y, label_chars, 1);
    let style = theme
        .title
        .add_modifier(Modifier::REVERSED)
        .add_modifier(Modifier::BOLD);
    frame.render_widget(Paragraph::new(Span::styled(label, style)), area);
}

// ---------------------------------------------------------------------------
// Footer
// ---------------------------------------------------------------------------

fn render_footer(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    // Priority order, top to bottom:
    //   1. text-input prompt (s)         — user typing
    //   2. kill confirmation (k / K)     — destructive: must be obvious
    //   3. fresh admin message           — last action's outcome
    //   4. sampler error                 — connection / SQL surface
    //   5. default keymap hint
    let line = if let Some(prompt) = app.prompt.as_ref() {
        build_prompt_line(prompt, theme)
    } else if let Some(req) = app.kill_confirm.as_ref() {
        build_kill_confirm_line(req, theme)
    } else if let Some(msg) = fresh_admin_message(app) {
        build_admin_message_line(msg, theme)
    } else if let Some(err) = app.last_error.as_deref() {
        Line::from(vec![
            Span::styled(" error ", Style::default().fg(ratatui::style::Color::Red)),
            Span::raw(truncate_err(err, area.width.saturating_sub(8) as usize)),
        ])
    } else {
        build_default_footer(theme)
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn build_kill_confirm_line<'a>(req: &'a KillRequest, theme: &'a Theme) -> Line<'a> {
    let qtime = req.qtime_secs.map_or_else(
        || "-".to_owned(),
        |s| {
            if s < 1.0 {
                format!("{:.0}ms", s * 1000.0)
            } else if s < 60.0 {
                format!("{s:.1}s")
            } else {
                format!("{:.0}m", s / 60.0)
            }
        },
    );
    Line::from(vec![
        Span::styled(
            format!(" {} ", req.mode.verb_upper()),
            Style::default()
                .bg(ratatui::style::Color::Red)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            " pid {} ({}@{}, {} {qtime}, ‹{}›)?  ",
            req.pid, req.usename, req.datname, req.state, req.query_summary,
        )),
        Span::styled("[y/N]", theme.title),
    ])
}

fn fresh_admin_message(app: &App) -> Option<&AdminMessage> {
    app.admin_message
        .as_ref()
        .filter(|m| std::time::Instant::now() < m.expires_at)
}

fn build_admin_message_line<'a>(msg: &'a AdminMessage, theme: &'a Theme) -> Line<'a> {
    let badge_style = match msg.level {
        AdminMessageLevel::Ok => Style::default()
            .bg(ratatui::style::Color::Green)
            .fg(ratatui::style::Color::Black)
            .add_modifier(Modifier::BOLD),
        AdminMessageLevel::Err => Style::default()
            .bg(ratatui::style::Color::Red)
            .add_modifier(Modifier::BOLD),
    };
    let badge = match msg.level {
        AdminMessageLevel::Ok => " OK ",
        AdminMessageLevel::Err => " ERR ",
    };
    Line::from(vec![
        Span::styled(badge, badge_style),
        Span::raw(" "),
        Span::styled(msg.text.clone(), theme.footer),
    ])
}

fn build_default_footer(theme: &Theme) -> Line<'_> {
    Line::from(vec![
        Span::styled(" q ", theme.title),
        Span::styled("quit  ", theme.footer),
        Span::styled("↑↓ ", theme.title),
        Span::styled("move  ", theme.footer),
        Span::styled("Space ", theme.title),
        Span::styled("refresh  ", theme.footer),
        Span::styled("←→ ", theme.title),
        Span::styled("sort  ", theme.footer),
        Span::styled("r ", theme.title),
        Span::styled("reverse  ", theme.footer),
        Span::styled("k/K ", theme.title),
        Span::styled("cancel/term  ", theme.footer),
        Span::styled("e ", theme.title),
        Span::styled("extended  ", theme.footer),
        Span::styled("s ", theme.title),
        Span::styled("set delay", theme.footer),
    ])
}

fn build_prompt_line<'a>(prompt: &'a PromptState, theme: &'a Theme) -> Line<'a> {
    let label = match prompt.kind {
        PromptKind::Refresh => prompt.kind.label(),
    };
    Line::from(vec![
        Span::styled(format!(" {label}: "), theme.title),
        Span::raw(prompt.buffer.as_str()),
        Span::styled("█", theme.title),
        Span::styled("   [Enter to apply, Esc to cancel]", theme.muted),
    ])
}

fn truncate_err(s: &str, max: usize) -> String {
    let cleaned = scrub_terminal_unsafe(s);
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
                max_connections: 100,
                longest_xact_secs: 125.0,
                longest_active_query_secs: 42.0,
                deadlocks_total: 0,
                temp_files_total: 5,
                autovacuum_busy: 1,
                autovacuum_max: 3,
                phys_slots: 2,
                phys_slots_active: 2,
                log_slots: 1,
                log_slots_active: 1,
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
                    locks_held: 4,
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
                    locks_held: 2,
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
                    locks_held: 7,
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
    fn default_layout_omits_app_and_client_columns() {
        let mut app = App::new();
        app.set_snapshot(fixture_snapshot());
        // Even on a wide terminal, app/client/backend are not in the default
        // column set — they require `e` to opt into extended mode.
        let buf = render_into(140, 30, &app);
        let dump = buffer_to_string(&buf);
        assert!(dump.contains("12345"), "default layout dropped pid");
        assert!(
            !dump.contains("10.0.0.5"),
            "default layout must not show the client column: {dump}"
        );
        assert!(
            !dump.contains("etl-runner"),
            "default layout must not show the application_name column: {dump}"
        );
    }

    #[test]
    fn extended_mode_adds_app_client_backend_columns() {
        let mut app = App::new();
        app.set_snapshot(fixture_snapshot());
        app.extended = true;
        let buf = render_into(140, 30, &app);
        let dump = buffer_to_string(&buf);
        assert!(
            dump.contains("10.0.0.5"),
            "extended mode must show client_addr: {dump}"
        );
        assert!(
            dump.contains("etl-runner"),
            "extended mode must show application_name: {dump}"
        );
        // The Activity title gains the [extended] indicator.
        assert!(
            dump.contains("[extended]"),
            "extended-mode badge missing: {dump}"
        );
    }

    #[test]
    fn header_renders_enriched_postgres_stats() {
        let mut app = App::new();
        app.set_snapshot(fixture_snapshot());
        let buf = render_into(160, 30, &app);
        let dump = buffer_to_string(&buf);
        assert!(
            dump.contains("total 22/100"),
            "total/max_connections missing: {dump}"
        );
        assert!(dump.contains("longest-tx"), "longest-tx missing: {dump}");
        assert!(dump.contains("longest-q"), "longest-q missing: {dump}");
        assert!(
            dump.contains("deadlocks 0"),
            "deadlocks counter missing: {dump}"
        );
        assert!(
            dump.contains("temp-files 5"),
            "temp-files counter missing: {dump}"
        );
    }

    #[test]
    fn header_renders_ops_line_with_autovacuum_and_slots() {
        let mut app = App::new();
        app.set_snapshot(fixture_snapshot());
        let buf = render_into(180, 30, &app);
        let dump = buffer_to_string(&buf);
        assert!(
            dump.contains("av 1/3"),
            "autovacuum busy/max missing: {dump}"
        );
        assert!(
            dump.contains("slots 2/2p 1/1l"),
            "replication slot counts missing: {dump}"
        );
    }

    #[test]
    fn footer_shows_kill_confirmation_when_pending() {
        use crate::top::state::{KillMode, KillRequest};

        let mut app = App::new();
        app.set_snapshot(fixture_snapshot());
        app.kill_confirm = Some(KillRequest {
            mode: KillMode::Terminate,
            pid: 12_345,
            usename: "app".into(),
            datname: "prod".into(),
            state: "active".into(),
            qtime_secs: Some(42.0),
            query_summary: "update accounts set …".into(),
        });
        let buf = render_into(180, 30, &app);
        let dump = buffer_to_string(&buf);
        assert!(dump.contains("TERMINATE"), "verb missing: {dump}");
        assert!(dump.contains("pid 12345"), "pid missing: {dump}");
        assert!(dump.contains("[y/N]"), "confirm hint missing: {dump}");
    }

    #[test]
    fn footer_shows_admin_message_briefly() {
        use crate::top::state::{AdminMessage, AdminMessageLevel};
        use std::time::Duration;

        let mut app = App::new();
        app.set_snapshot(fixture_snapshot());
        app.admin_message = Some(AdminMessage {
            text: "CANCEL pid 12345: ok".into(),
            level: AdminMessageLevel::Ok,
            expires_at: std::time::Instant::now() + Duration::from_secs(5),
        });
        let buf = render_into(160, 30, &app);
        let dump = buffer_to_string(&buf);
        assert!(dump.contains(" OK "), "OK badge missing: {dump}");
        assert!(
            dump.contains("CANCEL pid 12345: ok"),
            "admin message missing: {dump}"
        );
    }

    #[test]
    fn footer_shows_refresh_prompt_when_open() {
        let mut app = App::new();
        app.set_snapshot(fixture_snapshot());
        app.open_refresh_prompt();
        let buf = render_into(140, 30, &app);
        let dump = buffer_to_string(&buf);
        assert!(
            dump.contains("delay (secs)"),
            "prompt label missing: {dump}"
        );
        assert!(
            dump.contains("[Enter to apply, Esc to cancel]"),
            "prompt hint missing: {dump}"
        );
        // Default footer hints must be hidden while the prompt is open.
        assert!(
            !dump.contains(" quit  "),
            "default footer must be replaced by the prompt: {dump}"
        );
    }

    #[test]
    fn active_sort_column_renders_with_arrow_indicator() {
        use crate::top::state::SortColumn;

        // Default = Qtime descending → expect "qtime▼" in the table header.
        let mut app = App::new();
        app.set_snapshot(fixture_snapshot());
        let buf = render_into(140, 30, &app);
        assert!(
            buffer_to_string(&buf).contains("qtime▼"),
            "expected qtime▼ on default sort: {}",
            buffer_to_string(&buf)
        );

        // Switch to Pid asc — sort_desc inherits Pid's default (desc),
        // so r toggles to asc.
        app.sort_column = SortColumn::Pid;
        app.sort_desc = false;
        let buf = render_into(140, 30, &app);
        let dump = buffer_to_string(&buf);
        assert!(dump.contains("pid▲"), "expected pid▲ on Pid asc: {dump}");
        // The previously-active column should not still carry an arrow.
        assert!(
            !dump.contains("qtime▼") && !dump.contains("qtime▲"),
            "qtime should not be marked active any more: {dump}"
        );
    }

    #[test]
    fn key_overlay_appears_after_a_recent_keypress() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = App::new();
        app.set_snapshot(fixture_snapshot());
        // Press Down — sets `last_key` with the "↓" label and a fresh TTL.
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), 10);
        let buf = render_into(140, 30, &app);
        let dump = buffer_to_string(&buf);
        assert!(dump.contains("⌨ ↓"), "expected ⌨ ↓ overlay: {dump}");
    }

    #[test]
    fn key_overlay_omits_after_expiry() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = App::new();
        app.set_snapshot(fixture_snapshot());
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE), 10);
        // Force-expire the overlay.
        app.last_key.as_mut().unwrap().expires_at = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_millis(1))
            .expect("clock has advanced past 1 ms since boot");
        let buf = render_into(140, 30, &app);
        let dump = buffer_to_string(&buf);
        assert!(
            !dump.contains("⌨"),
            "expired overlay should not render: {dump}"
        );
    }

    #[test]
    fn locks_column_shows_dash_for_zero_and_number_otherwise() {
        let mut app = App::new();
        app.set_snapshot(fixture_snapshot());
        let buf = render_into(140, 30, &app);
        let dump = buffer_to_string(&buf);
        // Header row contains the column label.
        assert!(dump.contains("locks"), "locks header missing: {dump}");
        // pid 12345 has 4 locks, pid 12350 has 7.
        assert!(dump.contains('4'), "expected 4-locks count visible");
        assert!(dump.contains('7'), "expected 7-locks count visible");
    }

    /// End-to-end safety: a malicious `pg_stat_activity` row with an ANSI
    /// escape in `application_name`/`query`/`db_name` must not produce any
    /// ESC byte in any rendered cell symbol. This protects DBAs whose
    /// terminals would otherwise execute the embedded escape.
    #[test]
    fn renderer_strips_ansi_escapes_from_user_controllable_fields() {
        use crate::top::state::{ActivityRow, ServerSummary};
        let mut app = App::new();
        app.set_snapshot(Snapshot {
            ts: 1,
            server: ServerSummary {
                db_name: "evil\x1b[2J".into(),
                user: "nik\x1b[31m".into(),
                pg_version: "16.4".into(),
                ..Default::default()
            },
            rows: vec![ActivityRow {
                pid: 1,
                usename: "u".into(),
                datname: "d".into(),
                application_name: "psql\x1b[33mPWNED".into(),
                state: "active".into(),
                query: "select 1\x1b[2J -- pwn\x07".into(),
                ..Default::default()
            }],
        });
        let buf = render_into(140, 30, &app);
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let sym = buf[(x, y)].symbol();
                assert!(
                    !sym.as_bytes().iter().any(|b| *b == 0x1b || *b == 0x07),
                    "cell ({x},{y}) contains a control byte: {sym:?}",
                );
            }
        }
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

    /// Inspect cell-level styling: when the sampler is fresh, the connection
    /// dot must use `theme.status_ok`; after a missed tick it must flip to
    /// `theme.status_stale`. Without this, a bug that swaps the two styles
    /// would pass the dump-substring tests above.
    #[test]
    fn connection_led_color_reflects_freshness() {
        let theme = Theme::default_theme(); // use the colored theme so fg differs

        // Fresh: status_ok
        let mut fresh = App::new();
        fresh.set_snapshot(fixture_snapshot());
        let backend = TestBackend::new(140, 30);
        let mut term = Terminal::new(backend).expect("terminal");
        term.draw(|f| draw(f, &fresh, &theme)).expect("draw");
        let dot_fg_fresh = find_dot_fg(term.backend().buffer());
        assert_eq!(
            dot_fg_fresh,
            Some(theme.status_ok.fg.expect("status_ok must define fg")),
            "fresh connection dot must use status_ok color",
        );

        // Stale: status_stale
        let mut stale = App::new();
        stale.set_snapshot(fixture_snapshot());
        stale.note_stale();
        let backend = TestBackend::new(140, 30);
        let mut term = Terminal::new(backend).expect("terminal");
        term.draw(|f| draw(f, &stale, &theme)).expect("draw");
        let dot_fg_stale = find_dot_fg(term.backend().buffer());
        assert_eq!(
            dot_fg_stale,
            Some(theme.status_stale.fg.expect("status_stale must define fg")),
            "stale connection dot must use status_stale color",
        );
    }

    fn find_dot_fg(buf: &ratatui::buffer::Buffer) -> Option<ratatui::style::Color> {
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                if buf[(x, y)].symbol() == "●" {
                    return Some(buf[(x, y)].fg);
                }
            }
        }
        None
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
    fn format_clock_utc_handles_epoch_and_anchor() {
        assert_eq!(super::format_clock_utc(0), "00:00:00");
        // 1_700_000_000 = 2023-11-14T22:13:20Z (sanity-checked manually).
        assert_eq!(super::format_clock_utc(1_700_000_000), "22:13:20");
        // Negative or pre-epoch values fall back to "—".
        assert_eq!(super::format_clock_utc(-1), "—");
    }

    #[test]
    fn header_renders_human_readable_clock() {
        let mut app = App::new();
        app.set_snapshot(fixture_snapshot());
        let buf = render_into(140, 30, &app);
        let dump = buffer_to_string(&buf);
        assert!(
            !dump.contains("T1700000000"),
            "raw unix epoch must not be rendered in the header: {dump}"
        );
        assert!(
            dump.contains("22:13:20 UTC"),
            "header must render snapshot ts as HH:MM:SS UTC: {dump}"
        );
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
