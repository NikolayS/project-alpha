//! Activity view — one row per non-rpg backend from `pg_stat_activity`.
//!
//! Layout responds to the available width: a compact 80-column layout drops
//! `application_name`, `client_addr`, and `backend_type` so that the query
//! column always stays visible. Wider terminals (≥120 cols) restore them.

use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Row, Table};
use ratatui::Frame;

use crate::top::state::{ActivityRow, App, Snapshot};
use crate::top::theme::Theme;

/// Render the activity table into `area`. The caller is responsible for
/// passing only the body rectangle (header/tabs/footer are drawn outside).
pub fn render(frame: &mut Frame, area: Rect, app: &App, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(theme.border)
        .title(Line::from(vec![
            Span::styled(" Activity ", theme.title),
            Span::styled(row_count_caption(app.snapshot.as_ref()), theme.muted),
        ]));

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
    let wide = area.width >= 120;

    let header_cells = if wide {
        vec![
            "pid", "user", "db", "app", "client", "backend", "state", "wait", "qtime", "xtime",
            "query",
        ]
    } else {
        vec![
            "pid", "user", "db", "state", "wait", "qtime", "xtime", "query",
        ]
    };

    let widths: Vec<Constraint> = if wide {
        vec![
            Constraint::Length(7),  // pid
            Constraint::Length(10), // user
            Constraint::Length(12), // db
            Constraint::Length(12), // app
            Constraint::Length(15), // client
            Constraint::Length(10), // backend_type (e.g. "client", "parallel")
            Constraint::Length(8),  // state
            Constraint::Length(20), // wait
            Constraint::Length(7),  // qtime
            Constraint::Length(7),  // xtime
            Constraint::Min(30),    // query (flex)
        ]
    } else {
        vec![
            Constraint::Length(7),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Length(18),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Min(20),
        ]
    };

    let header = Row::new(
        header_cells
            .into_iter()
            .map(|h| Cell::from(Span::styled(h, theme.header)))
            .collect::<Vec<_>>(),
    )
    .height(1)
    .style(theme.header_row);

    let rows: Vec<Row> = snap
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| build_row(r, i == app.selected_row, wide, theme))
        .collect();

    let table = Table::new(rows, widths).header(header).column_spacing(1);
    frame.render_widget(table, area);
}

fn build_row<'a>(r: &'a ActivityRow, selected: bool, wide: bool, theme: &'a Theme) -> Row<'a> {
    let state_style = state_color(&r.state, theme);
    let wait_style = wait_color(&r.wait_event_type, theme);
    let qtime_style = qtime_warn_color(r.qtime_secs, theme);

    let wait = if r.wait_event_type.is_empty() {
        String::new()
    } else if r.wait_event.is_empty() {
        r.wait_event_type.clone()
    } else {
        format!("{}.{}", r.wait_event_type, r.wait_event)
    };

    let cells: Vec<Cell> = if wide {
        vec![
            Cell::from(r.pid.to_string()),
            Cell::from(truncate(&r.usename, 10)),
            Cell::from(truncate(&r.datname, 12)),
            Cell::from(truncate(&r.application_name, 12)),
            Cell::from(truncate(&r.client_addr, 15)),
            Cell::from(truncate(&short_backend_type(&r.backend_type), 10)),
            Cell::from(Span::styled(truncate(&r.state, 8), state_style)),
            Cell::from(Span::styled(truncate(&wait, 20), wait_style)),
            Cell::from(Span::styled(format_secs(r.qtime_secs), qtime_style)),
            Cell::from(format_secs(r.xtime_secs)),
            Cell::from(squash_query(&r.query)),
        ]
    } else {
        vec![
            Cell::from(r.pid.to_string()),
            Cell::from(truncate(&r.usename, 10)),
            Cell::from(truncate(&r.datname, 10)),
            Cell::from(Span::styled(truncate(&r.state, 8), state_style)),
            Cell::from(Span::styled(truncate(&wait, 18), wait_style)),
            Cell::from(Span::styled(format_secs(r.qtime_secs), qtime_style)),
            Cell::from(format_secs(r.xtime_secs)),
            Cell::from(squash_query(&r.query)),
        ]
    };

    let row = Row::new(cells);
    if selected {
        row.style(theme.selected)
    } else {
        row
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn squash_query(s: &str) -> String {
    let collapsed: String = s
        .chars()
        .map(|c| {
            if c == '\n' || c == '\r' || c == '\t' {
                ' '
            } else {
                c
            }
        })
        .collect();
    collapsed.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn format_secs(v: Option<f64>) -> String {
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

fn state_color(state: &str, theme: &Theme) -> Style {
    match state {
        "active" => theme.state_active,
        "idle in transaction" | "idle in transaction (aborted)" => theme.state_idle_in_tx,
        "idle" => theme.muted,
        _ => Style::default(),
    }
}

fn wait_color(wtype: &str, theme: &Theme) -> Style {
    match wtype {
        "Lock" => theme.wait_lock,
        "LWLock" => theme.wait_lwlock,
        "IO" => theme.wait_io,
        _ => Style::default(),
    }
}

/// Compact form of `pg_stat_activity.backend_type`. Most users only care
/// whether a row is a regular client backend or a parallel/maintenance
/// helper, so we strip the trailing " backend" suffix and shorten
/// "background" to keep the column narrow.
fn short_backend_type(s: &str) -> String {
    s.replace(" backend", "").replace("background ", "bg ")
}

/// Threshold-based coloring for elapsed query time.
/// Hard-coded thresholds for S1; later sprints read from `[top.thresholds]`.
fn qtime_warn_color(secs: Option<f64>, theme: &Theme) -> Style {
    match secs {
        Some(s) if s >= 30.0 => theme.wait_lock,       // crit: red
        Some(s) if s >= 1.0 => theme.state_idle_in_tx, // warn: yellow
        _ => Style::default(),
    }
}

/// Public helper — used by the kill-confirm modal in later sprints.
#[allow(dead_code)]
pub const SELECTED_HIGHLIGHT: Style = Style::new()
    .add_modifier(Modifier::REVERSED)
    .fg(Color::White);

#[cfg(test)]
mod tests {
    use super::*;

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
        // 6 wide chars; max = 4 → 3 chars + …
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
}
