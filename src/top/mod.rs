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

/// Parsed `/top` invocation flags.
#[derive(Debug, Default, Clone)]
pub struct TopArgs {
    /// Headless mode: take one snapshot, dump a plain-text rendering to
    /// stdout, and exit. Skips the alt-screen + raw-mode setup so it is
    /// safe to run from `rpg --command "/top --once"` and from CI.
    pub once: bool,
    /// Continuous batch logging: print a timestamped snapshot every
    /// `refresh_secs` until interrupted. Designed for `tmux pipe-pane` /
    /// shell redirection. Skips alt-screen + raw-mode entirely.
    pub batch: bool,
    /// Sampler refresh interval in seconds. `None` keeps
    /// [`state::DEFAULT_REFRESH_SECS`]. CLI: `--refresh <n>` or `-s <n>`,
    /// matching `pg_top` / `pgcenter`.
    pub refresh_secs: Option<f64>,
    /// Strftime-style format used for the timestamp prefix in `--batch`
    /// mode. `None` uses [`state::DEFAULT_TS_FORMAT`] (ISO 8601 UTC).
    pub ts_format: Option<String>,
}

impl TopArgs {
    /// Parse the argument string passed after `/top` in the REPL (or via
    /// `rpg --command`). Unknown tokens are silently ignored.
    pub fn parse(args: &str) -> Self {
        use state::{MAX_REFRESH_SECS, MIN_REFRESH_SECS};

        let mut out = Self::default();
        let mut iter = args.split_whitespace().peekable();
        while let Some(tok) = iter.next() {
            match tok {
                "--once" => out.once = true,
                "--batch" | "-b" => out.batch = true,
                "--refresh" | "-s" => {
                    if let Some(val) = iter.next() {
                        if let Ok(n) = val.parse::<f64>() {
                            if (MIN_REFRESH_SECS..=MAX_REFRESH_SECS).contains(&n) {
                                out.refresh_secs = Some(n);
                            }
                        }
                    }
                }
                "--ts-format" => {
                    if let Some(val) = iter.next() {
                        out.ts_format = Some(val.to_owned());
                    }
                }
                _ => {}
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
    if args.batch {
        return run_batch(client, &args).await;
    }

    if !io::stdout().is_terminal() {
        anyhow::bail!(
            "/top requires an interactive terminal (use `--once` for a snapshot, \
             `--batch` for continuous logging)"
        );
    }

    let mut app = App::new();
    if let Some(secs) = args.refresh_secs {
        app.refresh_secs = secs;
    }
    let theme = Theme::default_theme();

    let _guard = TerminalGuard::new()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    'outer: loop {
        // 1. Sampler tick.
        app.force_refresh = false;
        match sampler::tick(client, SAMPLE_TIMEOUT_MS).await {
            Ok(TickResult::Ok(snap)) => app.set_snapshot(*snap),
            Ok(TickResult::Missed) => app.note_stale(),
            Err(e) => app.note_error(format!("{e}")),
        }

        // 2. Draw frame.
        terminal.draw(|f| renderer::draw(f, &app, &theme))?;

        // 3. Drain key/mouse events until the next refresh deadline.
        // The deadline is recomputed each tick so an interactive change to
        // `app.refresh_secs` (via the `s` prompt) takes effect immediately.
        // `Space` short-circuits the wait by setting `app.force_refresh` —
        // matches `top`'s "redisplay now" convention.
        let deadline = Instant::now() + Duration::from_secs_f64(app.refresh_secs);
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
                    if app.force_refresh {
                        break;
                    }
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

/// Off-screen buffer dimensions used by `--once` and `--batch`. Wide
/// enough to render the default-mode columns + a trimmed query string.
const HEADLESS_WIDTH: u16 = 130;
const HEADLESS_HEIGHT: u16 = 30;

/// Headless `--once` mode: take one snapshot, render into a fixed-size
/// off-screen buffer, write the cell contents to stdout as plain text, and
/// exit. Used for scripting, CI smoke tests, and PR evidence capture.
async fn run_once(client: &Client) -> anyhow::Result<()> {
    let mut app = App::new();
    sample_into_app(client, &mut app).await;
    write_text_frame(&app, io::stdout().lock())?;
    Ok(())
}

/// Continuous `--batch` mode: print a timestamped snapshot every
/// `refresh_secs` to stdout until SIGINT. Designed for tmux `pipe-pane`,
/// shell redirection (`> top.log`), and other long-running log capture
/// workflows. Skips alt-screen + raw-mode entirely so the output is plain
/// text. Each snapshot is preceded by a separator line containing the
/// strftime-formatted timestamp, e.g.
///
/// ```text
/// ===== 2026-05-08T12:13:14Z =====
/// ┌ rpg /top ────────…
/// …
/// ```
async fn run_batch(client: &Client, args: &TopArgs) -> anyhow::Result<()> {
    use std::time::Duration;

    use state::{format_strftime, DEFAULT_REFRESH_SECS, DEFAULT_TS_FORMAT};

    let interval = Duration::from_secs_f64(args.refresh_secs.unwrap_or(DEFAULT_REFRESH_SECS));
    let fmt = args
        .ts_format
        .as_deref()
        .unwrap_or(DEFAULT_TS_FORMAT)
        .to_owned();

    let mut interrupt = std::pin::pin!(tokio::signal::ctrl_c());

    loop {
        let mut app = App::new();
        sample_into_app(client, &mut app).await;
        let ts_secs = app.snapshot.as_ref().map_or(0, |s| s.ts);
        let ts = format_strftime(&fmt, ts_secs);
        let mut out = io::stdout().lock();
        writeln!(out, "===== {ts} =====")?;
        write_text_frame(&app, &mut out)?;
        writeln!(out)?;
        out.flush()?;
        drop(out);

        tokio::select! {
            biased;
            _ = &mut interrupt => break,
            () = tokio::time::sleep(interval) => {}
        }
    }
    Ok(())
}

async fn sample_into_app(client: &Client, app: &mut App) {
    match sampler::tick(client, SAMPLE_TIMEOUT_MS).await {
        Ok(TickResult::Ok(snap)) => app.set_snapshot(*snap),
        Ok(TickResult::Missed) => app.note_stale(),
        Err(e) => app.note_error(format!("{e}")),
    }
}

fn write_text_frame<W: Write>(app: &App, mut out: W) -> anyhow::Result<()> {
    use ratatui::backend::TestBackend;

    let theme = Theme::for_once();
    let backend = TestBackend::new(HEADLESS_WIDTH, HEADLESS_HEIGHT);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|f| renderer::draw(f, app, &theme))?;
    let buf = terminal.backend().buffer();
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

    #[test]
    fn parse_refresh_flag_accepts_values_in_range() {
        for (input, expected) in [
            ("--refresh 0.5", Some(0.5)),
            ("-s 2", Some(2.0)),
            ("--refresh 0.1", Some(0.1)),
            ("--refresh 60", Some(60.0)),
        ] {
            let a = TopArgs::parse(input);
            assert!(
                matches!(a.refresh_secs, x if (x.unwrap_or(-1.0) - expected.unwrap()).abs() < f64::EPSILON),
                "{input} → {:?}",
                a.refresh_secs
            );
        }
    }

    #[test]
    fn parse_refresh_flag_rejects_out_of_range() {
        for input in [
            "--refresh 0.05",
            "--refresh 999",
            "--refresh xx",
            "--refresh -1",
        ] {
            let a = TopArgs::parse(input);
            assert!(
                a.refresh_secs.is_none(),
                "{input} accepted: {:?}",
                a.refresh_secs
            );
        }
    }

    #[test]
    fn parse_refresh_combines_with_once() {
        let a = TopArgs::parse("--once --refresh 0.5");
        assert!(a.once);
        assert_eq!(a.refresh_secs, Some(0.5));
    }

    #[test]
    fn parse_batch_flag_sets_batch() {
        assert!(TopArgs::parse("--batch").batch);
        assert!(TopArgs::parse("-b").batch);
    }

    #[test]
    fn parse_ts_format_round_trip() {
        let a = TopArgs::parse("--batch --ts-format %Y-%m-%dT%H:%M:%SZ");
        assert!(a.batch);
        assert_eq!(a.ts_format.as_deref(), Some("%Y-%m-%dT%H:%M:%SZ"));
    }

    #[test]
    fn parse_batch_with_refresh_and_ts_format() {
        let a = TopArgs::parse("--batch --refresh 5 --ts-format %F-%T");
        assert!(a.batch);
        assert_eq!(a.refresh_secs, Some(5.0));
        assert_eq!(a.ts_format.as_deref(), Some("%F-%T"));
    }
}
