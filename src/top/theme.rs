//! Color palette for `/top`. The wait-event palette mirrors
//! [`pg_ash`](https://github.com/NikolayS/pg_ash) color scheme
//! (see `docs/COLOR_SCHEME.md` in the `pg_ash` repo) so muscle memory
//! transfers between the rpg terminal, `pg_ash` output, and the
//! `PostgresAI` Grafana dashboards.
//!
//! When the terminal advertises 24-bit truecolor (`COLORTERM=truecolor`)
//! we emit the exact `pg_ash` hex values; otherwise we fall back to
//! `ratatui`'s named colors. Palette source: `pg_ash` `COLOR_SCHEME.md`,
//! Dashboard 4 (Wait Sampling).

use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub border: Style,
    pub title: Style,
    pub muted: Style,
    pub header: Style,
    pub header_row: Style,
    pub selected: Style,
    pub footer: Style,
    pub status_ok: Style,
    pub status_stale: Style,

    /// State column coloring — visually distinct from wait-event coloring.
    pub state_active: Style,
    pub state_idle_in_tx: Style,

    /// Wait-event-type coloring. Matches `pg_ash`'s Dashboard 4 palette.
    pub wait_cpu: Style,
    pub wait_idle_tx: Style,
    pub wait_io: Style,
    pub wait_lock: Style,
    pub wait_lwlock: Style,
    pub wait_ipc: Style,
    pub wait_client: Style,
    pub wait_timeout: Style,
    pub wait_buffer_pin: Style,
    pub wait_activity: Style,
    pub wait_extension: Style,
    pub wait_other: Style,

    /// qtime warn/crit coloring (project-specific thresholds, not `pg_ash`).
    pub qtime_warn: Style,
    pub qtime_crit: Style,
}

impl Theme {
    pub fn default_theme() -> Self {
        let truecolor = terminal_has_truecolor();
        let cpu_green = rgb_or(truecolor, 0x50, 0xFA, 0x7B, Color::Green);
        let idle_tx_yellow = rgb_or(truecolor, 0xF1, 0xFA, 0x8C, Color::LightYellow);
        let io_blue = rgb_or(truecolor, 0x1E, 0x64, 0xFF, Color::Blue);
        let lock_red = rgb_or(truecolor, 0xFF, 0x55, 0x55, Color::Red);
        let lwlock_pink = rgb_or(truecolor, 0xFF, 0x79, 0xC6, Color::Magenta);
        let ipc_cyan = rgb_or(truecolor, 0x00, 0xC8, 0xFF, Color::Cyan);
        let client_yellow = rgb_or(truecolor, 0xFF, 0xDC, 0x64, Color::Yellow);
        let timeout_orange = rgb_or(truecolor, 0xFF, 0xA5, 0x00, Color::LightRed);
        let buffer_pin_teal = rgb_or(truecolor, 0x00, 0xD2, 0xB4, Color::Cyan);
        let activity_purple = rgb_or(truecolor, 0x96, 0x64, 0xFF, Color::Magenta);
        let extension_purple = rgb_or(truecolor, 0xBE, 0x96, 0xFF, Color::LightMagenta);
        let other_gray = rgb_or(truecolor, 0xB4, 0xB4, 0xB4, Color::Gray);

        Self {
            border: Style::default().fg(Color::DarkGray),
            title: Style::default().add_modifier(Modifier::BOLD),
            muted: Style::default().fg(Color::DarkGray),
            header: Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
            header_row: Style::default().bg(Color::Black),
            selected: Style::default().add_modifier(Modifier::REVERSED),
            footer: Style::default().fg(Color::Gray),
            status_ok: Style::default().fg(Color::Green),
            status_stale: Style::default().fg(Color::Yellow),

            // State column: keep distinct from wait coloring.
            // Active = CPU green; idle-in-tx = pg_ash IdleTx light yellow.
            state_active: Style::default().fg(cpu_green),
            state_idle_in_tx: Style::default().fg(idle_tx_yellow),

            // Wait-event-type colors per pg_ash COLOR_SCHEME.md.
            wait_cpu: Style::default().fg(cpu_green),
            wait_idle_tx: Style::default().fg(idle_tx_yellow),
            wait_io: Style::default().fg(io_blue),
            wait_lock: Style::default().fg(lock_red),
            wait_lwlock: Style::default().fg(lwlock_pink),
            wait_ipc: Style::default().fg(ipc_cyan),
            wait_client: Style::default().fg(client_yellow),
            wait_timeout: Style::default().fg(timeout_orange),
            wait_buffer_pin: Style::default().fg(buffer_pin_teal),
            wait_activity: Style::default().fg(activity_purple),
            wait_extension: Style::default().fg(extension_purple),
            wait_other: Style::default().fg(other_gray),

            // qtime warn/crit (rpg-specific, distinct from wait colors).
            qtime_warn: Style::default().fg(client_yellow),
            qtime_crit: Style::default().fg(lock_red),
        }
    }

    /// Plain-style theme used by `/top --once`. Strips colors so that piping
    /// to a file or grep produces clean text. Implementation reuses the
    /// `for_tests` constructor.
    pub fn for_once() -> Self {
        Self::plain()
    }

    /// Test-only constructor that ignores terminal capabilities. Snapshot
    /// tests use this so output is identical across machines.
    #[cfg(test)]
    pub fn for_tests() -> Self {
        Self::plain()
    }

    fn plain() -> Self {
        let s = Style::default();
        Self {
            border: s,
            title: s,
            muted: s,
            header: s,
            header_row: s,
            selected: s.add_modifier(Modifier::REVERSED),
            footer: s,
            status_ok: s,
            status_stale: s,
            state_active: s,
            state_idle_in_tx: s,
            wait_cpu: s,
            wait_idle_tx: s,
            wait_io: s,
            wait_lock: s,
            wait_lwlock: s,
            wait_ipc: s,
            wait_client: s,
            wait_timeout: s,
            wait_buffer_pin: s,
            wait_activity: s,
            wait_extension: s,
            wait_other: s,
            qtime_warn: s,
            qtime_crit: s,
        }
    }
}

/// Pick truecolor RGB when the terminal supports it, otherwise the
/// nearest ratatui named color.
fn rgb_or(truecolor: bool, r: u8, g: u8, b: u8, fallback: Color) -> Color {
    if truecolor {
        Color::Rgb(r, g, b)
    } else {
        fallback
    }
}

/// Truecolor detection — duplicates the small helper in
/// `src/ash/renderer.rs` so `/top` does not depend on `/ash` internals.
fn terminal_has_truecolor() -> bool {
    std::env::var("COLORTERM")
        .is_ok_and(|v| v.eq_ignore_ascii_case("truecolor") || v.eq_ignore_ascii_case("24bit"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_constructs_without_panic() {
        let t = Theme::default_theme();
        assert!(t.selected.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn for_tests_is_deterministic() {
        let a = Theme::for_tests();
        let b = Theme::for_tests();
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
    }

    #[test]
    fn pg_ash_palette_uses_truecolor_when_advertised() {
        // We can't easily set COLORTERM mid-process safely; instead probe the
        // helper directly.
        let truecolor = Color::Rgb(0x50, 0xFA, 0x7B);
        assert_eq!(rgb_or(true, 0x50, 0xFA, 0x7B, Color::Green), truecolor);
        assert_eq!(rgb_or(false, 0x50, 0xFA, 0x7B, Color::Green), Color::Green);
    }
}
