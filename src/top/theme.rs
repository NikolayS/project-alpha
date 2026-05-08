//! Color palette for `/top`. S1 ships a single default theme; later sprints
//! add a colorblind-safe variant and threshold-driven coloring per
//! `.rpg.toml`.
//!
//! Truecolor detection mirrors `/ash` (`src/ash/renderer.rs`): when the
//! terminal advertises 24-bit color we use rich RGB shades; otherwise we
//! fall back to ratatui's named colors which map to the standard 256-color
//! palette.

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
    pub state_active: Style,
    pub state_idle_in_tx: Style,
    pub wait_lock: Style,
    pub wait_lwlock: Style,
    pub wait_io: Style,
}

impl Theme {
    pub fn default_theme() -> Self {
        let truecolor = terminal_has_truecolor();
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
            state_active: Style::default().fg(if truecolor {
                Color::Rgb(0x6c, 0xc6, 0x44)
            } else {
                Color::Green
            }),
            state_idle_in_tx: Style::default().fg(Color::Yellow),
            wait_lock: Style::default().fg(Color::Red),
            wait_lwlock: Style::default().fg(Color::LightYellow),
            wait_io: Style::default().fg(if truecolor {
                Color::Rgb(0x4f, 0x9c, 0xff)
            } else {
                Color::Blue
            }),
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
        Self {
            border: Style::default(),
            title: Style::default(),
            muted: Style::default(),
            header: Style::default(),
            header_row: Style::default(),
            selected: Style::default().add_modifier(Modifier::REVERSED),
            footer: Style::default(),
            status_ok: Style::default(),
            status_stale: Style::default(),
            state_active: Style::default(),
            state_idle_in_tx: Style::default(),
            wait_lock: Style::default(),
            wait_lwlock: Style::default(),
            wait_io: Style::default(),
        }
    }
}

/// Truecolor detection — duplicates the small helper in
/// `src/ash/renderer.rs` so `/top` does not depend on `/ash` internals.
/// Trades a tiny duplication for module isolation.
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
        // Selected style must be visually distinct from unselected.
        assert!(t.selected.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn for_tests_is_deterministic() {
        let a = Theme::for_tests();
        let b = Theme::for_tests();
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
    }
}
