//! `/top` — live TUI Postgres monitor (S1: scaffold + activity view).
//!
//! Mirrors the `/ash` module layout (`mod.rs`, `state.rs`, `sampler.rs`,
//! `renderer.rs`) so reviewers can pattern-match between the two.
//!
//! ```text
//! mod.rs         — public entry point: run_top(); event/render loop.
//! state.rs       — App, View, Snapshot, ActivityRow.
//! sampler.rs     — Postgres-side data: server summary + pg_stat_activity.
//! sql.rs         — version-gated SQL strings.
//! renderer.rs    — ratatui frame: header / tabs / body / footer.
//! views/         — one file per view; S1 ships only `activity`.
//! theme.rs       — palette + truecolor detection.
//! ```
//!
//! S1 scope is intentionally small: a single Activity view with live
//! refresh and `q`/`Esc`/`Ctrl-C` exit. View switching, drill-down, kill,
//! sparklines, and snapshot export land in S2–S7 per
//! `.samo/spec/top/SPEC.md`.

pub mod renderer;
pub mod sampler;
pub mod sql;
pub mod state;
pub mod theme;
pub mod views;

use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio_postgres::Client;

use sampler::TickResult;
use state::App;
use theme::Theme;

use crate::repl::ReplSettings;

/// Default refresh interval. Configurable via `[top] refresh` in `.rpg.toml`
/// once that surface lands; for S1 it is hard-coded to 1 s.
const DEFAULT_REFRESH: Duration = Duration::from_secs(1);

/// Per-query observer-effect timeout. Mirrors `/ash`'s default sample
/// timeout.
const SAMPLE_TIMEOUT_MS: u64 = 5_000;

// ---------------------------------------------------------------------------
// TerminalGuard — RAII alt-screen + raw-mode wrapper.
//
// Same pattern as `src/ash/mod.rs`. Drop runs even on panic so the user's
// terminal is always restored.
// ---------------------------------------------------------------------------

struct TerminalGuard;

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = stdout.write_all(b"\x1b[H\x1b[2J\x1b[H");
        let _ = stdout.flush();
        let _ = io::stderr().write_all(b"\x1b[r");
        let _ = io::stderr().flush();
    }
}

/// Parsed `/top` invocation flags. S1 only ships `--once`; later sprints
/// add `--view`, `--filter`, `--refresh`, etc. (see `.samo/spec/top/SPEC.md`).
#[derive(Debug, Default, Clone, Copy)]
pub struct TopArgs {
    /// Headless mode: take one snapshot, dump a plain-text rendering to
    /// stdout, and exit. Skips the alt-screen + raw-mode setup so it is
    /// safe to run from `rpg --command "/top --once"` and from CI.
    pub once: bool,
}

impl TopArgs {
    /// Parse the argument string passed after `/top` in the REPL (or via
    /// `rpg --command`). Unknown tokens are warned about on stderr in the
    /// caller; here we just succeed for the recognised ones.
    pub fn parse(args: &str) -> Self {
        let mut out = Self::default();
        for tok in args.split_whitespace() {
            if tok == "--once" {
                out.once = true;
            }
        }
        out
    }
}

/// Public entry point. Blocks until the user exits with `q`, `Esc`, or
/// `Ctrl-C`. The `_settings` parameter is currently unused; later sprints
/// read theme + refresh interval from it.
pub async fn run_top(
    client: &Client,
    _settings: &ReplSettings,
    args: TopArgs,
) -> anyhow::Result<()> {
    if args.once {
        return run_once(client).await;
    }

    if !io::stdout().is_terminal() {
        anyhow::bail!("/top requires an interactive terminal (use `--once` for a snapshot)");
    }

    let mut app = App::new();
    let theme = Theme::default_theme();

    let _guard = TerminalGuard::new()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    'outer: loop {
        // 1. Sampler tick.
        match sampler::tick(client, SAMPLE_TIMEOUT_MS).await {
            Ok(TickResult::Ok(snap)) => app.set_snapshot(*snap),
            Ok(TickResult::Missed) => app.note_stale(),
            Err(e) => app.note_error(format!("{e}")),
        }

        // 2. Draw frame.
        terminal.draw(|f| renderer::draw(f, &app, &theme))?;

        // 3. Drain key/mouse events until the next refresh deadline.
        let deadline = Instant::now() + DEFAULT_REFRESH;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            if !event::poll(remaining)? {
                break;
            }
            match event::read()? {
                Event::Key(key) => {
                    let page = page_size(&terminal);
                    if app.handle_key(key, page) {
                        break 'outer;
                    }
                    // Redraw after every state-changing key for snappy UX.
                    terminal.draw(|f| renderer::draw(f, &app, &theme))?;
                }
                Event::Resize(_, _) => {
                    terminal.draw(|f| renderer::draw(f, &app, &theme))?;
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn page_size<B: ratatui::backend::Backend>(terminal: &Terminal<B>) -> usize {
    let area = terminal.size().unwrap_or_default();
    // Leave room for header (3) + tabs (1) + table header (1) + footer (1) +
    // borders (2) ≈ 8 rows of chrome.
    let body = u32::from(area.height).saturating_sub(8);
    body.max(1) as usize
}

/// Headless `--once` mode: take one snapshot, render into a fixed-size
/// off-screen buffer, write the cell contents to stdout as plain text, and
/// exit. Used for scripting, CI smoke tests, and PR evidence capture.
async fn run_once(client: &Client) -> anyhow::Result<()> {
    use ratatui::backend::TestBackend;

    const ONCE_WIDTH: u16 = 130;
    const ONCE_HEIGHT: u16 = 30;

    let mut app = App::new();
    let theme = Theme::for_once();

    match sampler::tick(client, SAMPLE_TIMEOUT_MS).await {
        Ok(TickResult::Ok(snap)) => app.set_snapshot(*snap),
        Ok(TickResult::Missed) => app.note_stale(),
        Err(e) => app.note_error(format!("{e}")),
    }

    let backend = TestBackend::new(ONCE_WIDTH, ONCE_HEIGHT);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|f| renderer::draw(f, &app, &theme))?;

    let buf = terminal.backend().buffer();
    let mut out = io::stdout().lock();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            out.write_all(buf[(x, y)].symbol().as_bytes())?;
        }
        out.write_all(b"\n")?;
    }
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::TopArgs;

    #[test]
    fn parse_no_args_keeps_defaults() {
        let a = TopArgs::parse("");
        assert!(!a.once);
    }

    #[test]
    fn parse_once_flag_sets_once() {
        assert!(TopArgs::parse("--once").once);
        assert!(TopArgs::parse("  --once  ").once);
    }

    #[test]
    fn parse_unknown_flags_are_ignored() {
        let a = TopArgs::parse("--banana --once --carrot");
        assert!(a.once);
    }
}
