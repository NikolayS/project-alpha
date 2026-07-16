# /top — live TUI Postgres monitor

> Spec version: **v0.2** (open questions resolved) · Slug: `top` · Status: **ready for S1**
> Format: samospec ([NikolayS/samospec](https://github.com/NikolayS/samospec))
> Stack: Rust, ratatui 0.30, crossterm, tokio-postgres
> Target rpg release: next minor after v0.11.x

---

## 1. Goal & Why

**Goal.** Ship `/top`: a real-time, ratatui-powered TUI inside rpg that fuses the
best of `top(1)`, [`pg_top`](https://pg_top.gitlab.io/), and
[`pgcenter top`](https://github.com/lesovsky/pgcenter) into a single experience
a DBA reaches for during an incident — without leaving the rpg session.

**Why.**

- rpg already ships `/ash` (active session history) and `/dba` (one-shot
  diagnostics), but lacks the always-on, top-style monitor that DBAs expect as
  their first reflex. Today they shell out to `pgcenter` or `pg_top`. Closing
  that gap completes the "modern Postgres terminal" promise from
  `docs/blueprints/SPEC.md`.
- pg_top is unmaintained-ish (last release sparse, requires `pg_proctab`
  extension for remote OS stats); pgcenter is excellent but Linux-only for
  procfs and Go-binary separate from the user's psql session.
- rpg has the perfect substrate: an established ratatui flow (`/ash`),
  a tokio-postgres connection, schema-aware completion, AI hooks, and a
  single-binary distribution. `/top` is the natural next big feature.
- It must feel **cool** — modern color, sparklines, smooth refresh, mouse,
  fuzzy filter, drill-down overlays, blocking tree, and confirm-before-kill
  bulk admin actions. Not just a port; an upgrade.

**Non-goals (this spec).**

- Long-term recording (`/record`) or offline reports (`/report`) — separate spec.
- Wait-event sampling profiler (`/profile`) — separate spec, will share helpers.
- Replacing `/ash` (per-second active-session history) — `/ash` stays the
  history view; `/top` is the live now-view. They cross-link via hotkey.
- Replacing `/dba` one-shot tables — `/dba` stays the scriptable form; many
  views are shared via a common SQL crate.
- Linux-kernel-level metrics that require `pg_proctab` or a custom extension
  in v1. Server-side helpers are a v1.1 extension point (§4.10).

---

## 2. User Stories

Written from the operator's perspective. Each maps to acceptance tests in §5.

**US-1 · Incident triage.** *As an on-call DBA*, when an alert fires I run
`/top` and within 2 seconds see: active session count, top long-running
queries (sorted by `qtime` desc), wait-event distribution, blocked/blocker
chains, and a sparkline of TPS / deadlocks / temp-files for the last 60 s.

**US-2 · Drill into a pid.** *As a DBA*, I move the cursor to a row and press
`Enter` (or click). An overlay opens with five tabs: **Query** (full text,
syntax-highlighted), **Plan** (`EXPLAIN`, no execute), **Explain Analyze**
(re-run with confirmation), **Locks** (held + waited), **Waits** (last-N
samples for this pid). `Esc` closes.

**US-3 · Switch view.** *As an operator*, I press `1`–`9` (or the named keys
`a / d / t / i / s / r / p / w / f`) to jump between **Activity, Databases,
Tables, Indexes, Statements, Replication, Progress, WAL, Functions** — all
backed by Postgres `pg_stat_*` views. The header re-fits, sort order remembers
per-view.

**US-4 · Sort & filter.** *As a DBA*, I press `o` to pick a sort column (or
click a column header), `O` to invert, and `/` to enter a fuzzy filter that
matches across query text, user, db, app_name, client_addr, state. `Esc`
clears.

**US-5 · Blocking tree.** *As an oncall*, I press `b` (or open the dedicated
view from `/top`) and see an ASCII tree of blocked → blocker chains, with
each node showing pid, user, state, wait_event, txn age, query summary.
Roots are blockers with no parent. The tree updates each refresh.

**US-6 · Bulk kill idle-in-transaction.** *As a DBA*, I press `K` and a modal
asks "cancel / terminate / dry-run", then "filter by state? (active /
idle-in-transaction / idle-in-transaction (aborted))", then "min duration?".
Confirmation lists pids that match before doing anything. `y` to commit, `n`
to cancel. Refusal-to-fire if &gt;200 pids match without `--force`.

**US-7 · Watch a long vacuum.** *As an operator*, I open the **Progress**
view; it streams `pg_stat_progress_vacuum / analyze / create_index / cluster
/ basebackup / copy` rows, each with a percent gauge, scanned/total blocks,
and ETA (linear extrapolation).

**US-8 · Standby lag.** *As an oncall*, I open the **Replication** view; it
shows per-standby application_name, state, sent/write/flush/replay LSN, and
lag in **bytes** *and* **time** (using `pg_last_wal_receive_lsn()` /
`replay_lag` columns). Replication slots' restart_lsn and active flag.

**US-9 · Pause & rewind.** *As a DBA*, I press `Space` to pause the refresh
loop. `[` and `]` step backward/forward through the in-memory ring buffer
(default 60 snapshots) so I can study a transient spike without losing it.

**US-10 · Snapshot export.** *As an SRE*, I press `S` to dump the current
visible state (header + active view + timestamp) to
`./rpg-top-YYYY-MM-DDTHH-MM-SSZ.{json,txt}` for sharing in a postmortem.

**US-11 · Batch mode.** *As a CI scripter*, I run `rpg --command "/top --once
--view activity --limit 20 --json"` and get a single JSON snapshot to stdout,
no TUI, suitable for piping into `jq`.

**US-12 · Cross-link to `/ash` and AI.** *As a DBA*, I press `Shift-A` to
open `/ash` zoomed on the selected pid's session (when pg_ash is available),
or `Shift-X` to send the selected query to `/explain` and get an AI-augmented
plan walkthrough.

**US-12b · AI overview of the whole view.** *As a DBA who just opened
`/top` mid-incident*, I press `Shift-I` ("Info") and rpg's AI reads the
current header, active view, sparklines, top-N rows, wait-event mix, and
any blocking chains, then produces a streaming Markdown summary like:
"17 active sessions, 3 idle-in-tx > 30 s. pid 12345 has held a lock for
17 m and is blocking 4 backends. Wait events are 60 % `IO.DataFileRead` —
buffer pressure. Suggested next steps: 1) inspect pid 12345 (Enter), 2)
check `shared_buffers`, 3) consider cancelling the idle-in-tx ring with
`K`." `Esc` closes; works offline-degraded (shows raw stats summary if no
AI provider is configured).

**US-13 · Theme & accessibility.** *As a colorblind user*, I set
`top.theme = "deuteranopia"` in `.rpg.toml` and the threshold colors switch
to a colorblind-safe palette. 24-bit truecolor when available, 256-color
indexed fallback (matches `/ash` policy in `src/ash/renderer.rs`).

**US-14 · Connection resilience.** *As a remote user*, when the DB connection
drops, the header turns red, the table freezes with a "stale 12s" badge, and
the sampler retries with exponential backoff (1s, 2s, 5s, 10s, capped). On
recovery, fresh data flows in without restart.

---

## 3. Architecture

`/top` mirrors `/ash`'s module layout (see `src/ash/{mod,renderer,sampler,
state}.rs`) so reviewers can pattern-match. New code lives under
`src/top/`.

<!-- architecture:begin -->
```
                                    ┌────────────────────────────────┐
                                    │  rpg REPL (src/repl/, metacmd) │
                                    │  /top dispatcher               │
                                    └──────────────┬─────────────────┘
                                                   │
                                                   ▼
                       ┌───────────────────────────────────────────────────┐
                       │                src/top/mod.rs                     │
                       │  • run_top(client, settings, args) -- inline loop │
                       │  • TUI lifecycle: enter raw + alt-screen          │
                       │  • crossterm event loop, redraw budget            │
                       └─────┬───────────────┬───────────────┬─────────────┘
                             │               │               │
                             ▼               ▼               ▼
                    ┌────────────────┐ ┌──────────────┐ ┌────────────────┐
                    │  state.rs      │ │  sampler.rs  │ │  renderer.rs   │
                    │  • App         │ │  inline tick │ │  ratatui Frame │
                    │  • View enum   │ │  ─ snapshot  │ │  per-view fns  │
                    │  • RingBuffer  │ │    every     │ │  Tabs, Table,  │
                    │  • Filter/Sort │ │    refresh_  │ │  Sparkline,    │
                    │  • KillSpec    │ │    interval  │ │  Gauge, Chart  │
                    │  • Theme       │ │  ─ DB stats  │ │  + Overlays    │
                    │  • Settings    │ │    only (v1) │ │                │
                    └──────┬─────────┘ └──────┬───────┘ └────────┬───────┘
                           │                  │                  │
                           ▼                  ▼                  ▼
                    ┌────────────────────────────────────────────────────┐
                    │                  src/top/views/                    │
                    │ activity.rs  databases.rs  tables.rs  indexes.rs   │
                    │ statements.rs  replication.rs  progress.rs         │
                    │ wal.rs  functions.rs  blocking.rs                  │
                    └─────────────────────┬──────────────────────────────┘
                                          │
                                          ▼
                    ┌────────────────────────────────────────────────────┐
                    │                  src/top/sql.rs                    │
                    │  Static SQL strings for each view, version-gated   │
                    │  (PG14..PG18). Reused by /dba where overlap.       │
                    └─────────────────────┬──────────────────────────────┘
                                          │
                                          ▼
                    ┌────────────────────────────────────────────────────┐
                    │            tokio-postgres connection               │
                    │       (existing rpg session connection reused)     │
                    └────────────────────────────────────────────────────┘
```
<!-- architecture:end -->

### 3.1 Components

- **`src/top/mod.rs`** — entry
  `run_top(client: &Client, settings: &ReplSettings, args: TopArgs) -> Result<()>`
  in S1 (mirrors `/ash`). Sets up alt-screen + raw mode (same RAII guard
  pattern as `src/ash/mod.rs:53`), runs the sample → draw → poll-events
  loop inline, restores terminal on drop or panic. Provides `--once`
  headless path that bypasses the loop and prints text.
  *Future evolution:* later sprints may move sampling to a separate
  `tokio::task` with an `Arc<Mutex<Client>>` once we add the drill-down
  sub-sampler in S4 (so the overlay can sample at a different rate without
  blocking the main view loop).
- **`src/top/state.rs`** — `App` struct: current `View`, `RingBuffer<Snapshot>`,
  `FilterState`, `SortState`, `KillSpec`, `Theme`, `Settings`, `LastError`.
  All UI is a pure projection of `App`.
- **`src/top/sampler.rs`** — runs each tick from inside `run_top`'s loop in
  S1 (the spec's separate-task design is deferred to a later sprint, see
  §3.1 above). Each tick: timestamp, run the always-on header SQL plus the
  active view's SQL, pack into a `Snapshot`, store on `App`. System stats
  (cpu/mem/io/net) are out of scope for v1 per §4.10.
- **`src/top/renderer.rs`** — ratatui `draw(frame: &mut Frame, app: &App)`.
  Layout: header bar (3 rows) → tabs (1 row) → body (flex) → footer (1 row).
  Body delegates to a per-view renderer.
- **`src/top/views/*.rs`** — one file per view. Each exports `sql(pg_version)
  -> &'static str`, `parse(rows) -> ViewData`, `render(frame, area, data,
  app)`. Adding a view is a single new file plus a `View` enum variant.
- **`src/top/overlay.rs`** — drill-down modal (Q / E / A / L / W tabs), kill
  confirmation modal, help (`?`), snapshot-export progress.
- **`src/top/sql.rs`** — co-locates SQL strings; tagged with the minimum PG
  version they support; uses `pg_stat_activity.backend_type` (PG10+) and
  `wait_event_type`/`wait_event` (PG9.6+); falls back gracefully when a
  column is missing.
- **`src/top/theme.rs`** — palette, threshold colors (warn/crit by metric),
  truecolor vs 256-color (reuse `terminal_has_truecolor` from
  `src/ash/renderer.rs:25`), colorblind variants.
- **`src/top/keys.rs`** — keymap as data: `KeyEvent → Action`. Loaded from
  defaults + optional `[top.keys]` table in `.rpg.toml`. Consistent with
  `/ash` muscle memory where overlapping (`q` quits, `?` help, `Esc` cancels).
- **`src/top/admin.rs`** — `pg_cancel_backend`/`pg_terminate_backend` runner
  with two-step confirm, dry-run mode, refusal threshold, and audit-log line
  printed to stderr after exit.

### 3.2 Data flow

1. `/top` dispatcher passes the existing rpg `&Client` straight to `run_top`.
   In S1 the loop is inline (sampler awaited, then redraw, then event poll);
   later sprints can lift sampling onto a separate `tokio::task` if the
   drill-down sub-sampler in S4 needs concurrent rates.
2. Each tick (default 1 s), the loop runs the active view SQL plus the
   header SQL (always-on summary). System stats (cpu/mem/io/net) are out of
   scope for v1 per §4.10.
3. Snapshot is stored on `App` (single source of truth for the renderer).
4. UI loop on every crossterm event or watch change calls
   `terminal.draw(|f| renderer::draw(f, &app))`. Frame budget &lt;16 ms; if a
   view query exceeds it, sampler dispatches in a separate `spawn_blocking`
   so the UI never stalls.
5. Pause (`Space`) freezes ingestion. `[`/`]` step the cursor through the
   ring buffer; renderer reads the chosen snapshot instead of head.

### 3.3 Trust boundaries & safety

- **Read-only by default.** No mutating SQL runs without an explicit kill or
  reset action initiated from the modal.
- **Confirm before kill.** Modal shows the exact pids and queries about to be
  affected. Bulk &gt;200 pids requires `--force` flag passed at launch.
- **Audit trail.** Every `pg_cancel_backend` / `pg_terminate_backend` is
  appended to rpg's session log (`src/logging.rs`), with operator, target
  pid, query digest, and outcome.
- **No shell escapes from the modal.** Filters and SQL parameters use
  parameterized queries.
- **Connection isolation.** Sampler and admin actions share the rpg session
  connection but always issue `SET LOCAL statement_timeout = '5s'` for
  sampler queries to avoid wedging on a hostile lock.

### 3.4 PostgreSQL compatibility (per `CLAUDE.md` matrix)

| Feature | PG14 | PG15 | PG16 | PG17 | PG18 |
|---|---|---|---|---|---|
| Activity / blocking / locks | ✅ | ✅ | ✅ | ✅ | ✅ |
| `pg_stat_statements` (extension) | ✅ | ✅ | ✅ | ✅ | ✅ |
| `pg_stat_progress_*` core 6 | ✅ | ✅ | ✅ | ✅ | ✅ |
| `pg_stat_wal` | ✅ | ✅ | ✅ | ✅ | ✅ |
| `pg_stat_io` | — | — | ✅ | ✅ | ✅ |
| `pg_stat_subscription_stats` | ✅ | ✅ | ✅ | ✅ | ✅ |

Version detection at startup; views that need newer columns are gated and
show a "requires PG ≥ 16" stub instead of a hard error.

---

## 4. Implementation Details

### 4.1 Command surface

Per `docs/COMMANDS.md`, `/top` is an rpg-specific extension. Inside the REPL:

```
/top                                   # interactive TUI, default view = activity
/top <view>                            # open directly to a view
                                       #   activity, databases, tables, indexes,
                                       #   statements, replication, progress, wal,
                                       #   functions, blocking
/top --refresh 2s                      # custom refresh interval
/top --no-color                        # disable colors (CI / pipes)
/top --once                            # one snapshot, exit
/top --json                            # implies --once, JSON to stdout
/top --view activity --limit 20        # batch-mode filter
/top --filter "state = 'active'"       # initial filter
/top --kill-allow-bulk                 # opt-in for bulk kill > 200 pids
/top --pid 12345                       # open drill-down on a pid
```

CLI is also reachable from outside the REPL via the existing
`rpg --command "/top --once …"` path (see `src/main.rs`), making US-11 work.

**Bare alias outside the REPL.** `rpg top [args...]` is a thin alias for
`rpg --command "/top args..."`, registered in the clap subcommand table next
to the existing rpg sub-commands. Mirrors `psql -c "\l"` muscle memory and
removes a quoting layer for shell users.

### 4.2 Layout (default 80×24)

```
┌─ rpg /top ─ db=prod  user=nik  pg=16.4  uptime=14d ─ load 0.42 0.31 0.28 ─┐
│ active 17  idle-in-tx 3  waiting 2  TPS  ▁▂▃▅▇█▆▄▂▁  deadlocks  · · · · 1 │
│ cpu 23%  mem 72%  io 18 MiB/s  net 4.2 MiB/s            connection ●  rt  │
├──────────────────────────────────────────────────────────────────────────┤
│ [1]Activity  [2]Db  [3]Tables  [4]Idx  [5]Stmts  [6]Repl  [7]Prog  [?]   │
├──────────────────────────────────────────────────────────────────────────┤
│ pid    user    db        state    wait_event       qtime   xtime  query  │
│ 12345  app     prod      active   IO.DataFileRead   42 s   42 s   UPDATE…│
│ 12346  etl     analytics active   Lock.transactionid 2.3s  17 m   SELECT…│
│ ...                                                                      │
├──────────────────────────────────────────────────────────────────────────┤
│ q quit  ↑↓ move  ⏎ drill  / filter  o sort  Space pause  K kill  ? help  │
└──────────────────────────────────────────────────────────────────────────┘
```

Wider terminals get more columns (client_addr, application_name, locks,
transaction id, backend_type). Narrow terminals collapse to pid/qtime/query.

### 4.3 ratatui specifics

- **Widgets used:** `Tabs`, `Table` (with `Row::new`/`Cell`), `Paragraph`,
  `Sparkline`, `Gauge`, `Chart`/`Dataset` (for the wait-event distribution
  bar), `Block`/`Borders`, `List` for help and dropdowns. Custom mini-widgets
  for the lock blocking tree and the "lag in bytes/time" two-cell gauge.
- **Async + crossterm:** event polling on a dedicated thread bridged to a
  `mpsc` channel (same pattern as `src/ash/mod.rs:112`). Render at most once
  per 16 ms (60 FPS cap) regardless of event volume.
- **Mouse:** `crossterm::event::EnableMouseCapture`. Click on tab → switch
  view. Click on column header → sort. Click on row → select. Drag on
  sparkline → time-cursor in pause mode.
- **Truecolor:** reuse `terminal_has_truecolor()` from `src/ash/renderer.rs:25`.
  Theme provides `(rgb, idx256)` pairs and picks at draw time.
- **Min terminal size:** 24 rows × 80 cols. Below that, a centered "terminal
  too small" panel like `src/ash/renderer.rs:1463`.

### 4.4 Refresh & throttling

- Default `refresh_interval = 1s`, configurable in `.rpg.toml`
  `[top] refresh = "1s"`, runtime via `r` (prompt) or `+`/`-` ±0.5 s.
- Auto-throttle: if the previous tick took &gt;500 ms, double the interval
  until it recovers; surface as a yellow badge "throttled 4s".
- Pause when the rpg pane loses focus (terminal `FocusLost` event) — opt-in
  via `[top] pause_on_blur = true`.

### 4.5 Filtering & sort

- `/` opens a fuzzy filter prompt at the footer. Match algorithm:
  case-insensitive substring across (user, db, state, wait_event, app_name,
  client_addr, query). Backslash-escaped `state:active` syntax for column-
  scoped match (`state:active wait:Lock`).
- `o` opens a sort-column picker (List widget). `O` toggles ascending /
  descending. Sort state persists per-view in `App`.
- Filter and sort survive view switches when the column exists on the new
  view; otherwise they reset with a one-line toast.

### 4.6 Drill-down overlay

- Layout: 80% × 80% centered popup, 5 tabs.
  1. **Query** — full text from `pg_stat_activity.query`, syntax-highlighted
     using rpg's existing highlighter (`src/highlight.rs`).
  2. **Plan** — `EXPLAIN` of the query (no execute). Cached for 5 s.
  3. **Plan+Analyze** — `EXPLAIN ANALYZE` (re-runs!). Yellow warning,
     requires `y`-confirm. Disabled when query is not a plain `SELECT`/CTE.
  4. **Locks** — held + waited locks for this pid via `pg_locks` joined to
     `pg_class` for relation names; mode, granted, transactionid.
  5. **Waits** — last 60 wait-event samples for this pid (1 Hz polling),
     rendered as a stacked bar like `/ash`'s view.
- `Tab` / `Shift-Tab` move between tabs. `Esc` closes.

### 4.7 Blocking tree (US-5)

- SQL: `pg_blocking_pids()` per active row, then a recursive walk in Rust
  to build the forest. (Pure-SQL recursion via CTE is also viable but the
  Rust walk lets us inject query-summary and wait-event nicely.)
- Render: indented ASCII tree using `└─` / `├─` / `│ ` (matches `tree(1)`).
  Node line: `pid  state  wait  txn_age  user@db  : query_summary`.
- Cycles defended: track visited pids; cap depth at 32 with a `…` truncation
  marker.

### 4.8 Bulk admin actions (US-6)

- Trigger keys: `k` (cancel single, prompt pid), `K` (bulk modal).
- Modal flow:
  1. Action: cancel | terminate | dry-run.
  2. Filter: state = active | idle in transaction | idle in transaction
     (aborted) | idle &gt; N | matches current `/` filter.
  3. Min duration in seconds.
  4. Confirmation panel listing matched rows (pid, user, db, state, age,
     query summary), capped at 50 rows preview, with `(+N more)`.
  5. `y` to fire, `n` to cancel. If &gt;200 rows and `--kill-allow-bulk` not
     passed, refuse and instruct.
- Audit line example:
  `[2026-05-12T14:33:11Z] /top kill nik@prod terminate state=idle-in-tx
   min_age=30s matched=12 succeeded=12 failed=0`.

### 4.9 Configuration (`.rpg.toml` extension)

```toml
[top]
refresh = "1s"
default_view = "activity"
hide_idle = true
theme = "default"            # "default" | "dark" | "light" | "deuteranopia"
pause_on_blur = false
ringbuffer_size = 60         # snapshots kept for rewind
sparkline_window = "60s"

[top.thresholds]
qtime_warn_s = 1.0
qtime_crit_s = 30.0
xtime_warn_s = 60.0
xtime_crit_s = 600.0
locks_warn = 5
locks_crit = 50

[top.keys]
# overrides; full default list in `src/top/keys.rs`
quit = "q"
help = "?"
filter = "/"
sort = "o"
kill = "k"
kill_bulk = "K"
ai_explain = "X"      # Shift-X — eXplain selected pid via AI
ai_info    = "I"      # Shift-I — Info overview of whole /top view via AI
mouse = true
```

### 4.10 System stats — out of scope for v1

Per the v0.2 review, **no procfs reader and no server-side helpers in v1**.
The header shows DB-side stats only (active sessions, TPS, deadlocks, temp
files, replication lag) and intentionally leaves the cpu/mem/io/net cells
blank with a one-line "system stats: not collected" caption. Reintroduction
is a separate spec — likely `.samo/spec/top-system-stats/` — and is a clean
additive change because `Snapshot` already has the optional fields.

### 4.11 AI hand-offs

Two AI keys, both gated on rpg's existing AI provider configuration
(`/budget`, `/clear`, `/ask` already wired). When no provider is configured,
each key shows a help line pointing at `/init`.

- **`Shift-X` — eXplain selection.** Sends the selected pid's row plus
  the current `Plan` (cached EXPLAIN, not ANALYZE) to `/explain`. Streams
  the response into the right half of the drill-down overlay so the user
  can read the plan and the AI commentary side-by-side. Cancel with `Esc`.
  Mnemonic matches the existing rpg `/explain` slash-command.
- **`Shift-I` — Info on whole view.** Packages the header (DB summary,
  load, sparklines), the active view name, current sort/filter, top-N
  rows (default 20, configurable), wait-event mix, and the blocking-tree
  summary into a structured prompt and streams a Markdown answer into a
  bottom-up panel. Includes "next-step" hints the operator can act on.
  Has `--ai-info` batch flag for `--once --json` mode (returns
  `{view: ..., info_markdown: ...}`).

Both hand-offs reuse `src/repl/ai_commands.rs` plumbing; nothing new in
the AI surface, only new entry points. The packaged context is capped at
~2k tokens by sampling rows and truncating long queries (with a
"truncated" marker).

### 4.12 WASM target

`/top` is gated `#[cfg(not(target_arch = "wasm32"))]` like `/ash` and `/rpg`.
WASM users get a friendly stub (mirrors `src/repl/ai_commands.rs:530`).

### 4.13 Mouse default

Mouse is **on by default** (v0.2 decision). `crossterm::EnableMouseCapture`
is set during alt-screen entry. Terminals that don't support mouse simply
ignore the escape sequences. Opt-out: `[top] mouse = false` in `.rpg.toml`
or `--no-mouse` at launch (also useful for users who want native terminal
copy/paste, which mouse capture intercepts).

---

## 5. Tests Plan (red/green TDD)

Per `CLAUDE.md`, every fix gets a failing test first. New features here are
tested via four layers, each with concrete files and runners.

### 5.1 Unit tests (Rust `#[cfg(test)]`)

- `src/top/state.rs`: filter parsing (`state:active wait:Lock`), sort-state
  invariants, ring-buffer wrap, kill-spec validation, threshold mapping.
- `src/top/sql.rs`: per-PG-version SQL string compiles and round-trips
  through `tokio-postgres::statement::Statement::parse_for(...)` (no DB).
- `src/top/keys.rs`: keymap merge (defaults vs user overrides) and conflict
  detection.

### 5.2 Snapshot rendering tests (`insta`)

Add `insta` to `[dev-dependencies]`. Tests build an `App` with fixture
snapshots and call `renderer::draw` into a fixed-size `TestBackend`
(`ratatui::backend::TestBackend`), then assert against committed snapshots.

```rust
#[test]
fn renders_activity_view_default() {
    let mut term = Terminal::new(TestBackend::new(120, 30)).unwrap();
    let app = App::with_snapshot(fixture("activity_busy.json"));
    term.draw(|f| renderer::draw(f, &app)).unwrap();
    insta::assert_snapshot!(term.backend());
}
```

Cover: each view × (light/dark theme) × narrow/wide layout × stale-data
badge × kill-confirm modal × help overlay × empty-state.

### 5.3 Property tests (`proptest`)

- Sort: round-trip `sort(filter(snapshot)) == filter(sort(snapshot))` (when
  filter is not column-scoped). Stable sort preserves insertion order on ties.
- Ring buffer: `push` then `cursor_at(-i)` returns the i-th-latest item for
  any `i ≤ size`.
- Filter parser: any string parses to a non-crashing `FilterSpec`.

### 5.4 Integration tests (gated by `--features integration`, real PG)

CI already runs an embedded Postgres matrix (PG14–PG18) — see `.github/
workflows/`. Add:

- `tests/top_smoke.rs`: spin up rpg against the test cluster, run
  `/top --once --json` for each view, assert non-empty JSON with the
  expected schema.
- `tests/top_blocking.rs`: open two sessions, deliberately create a
  blocker → blocked pair, run `/top blocking --once`, assert the tree has
  exactly one root and one leaf with matching pids.
- `tests/top_kill.rs`: spawn a long-running `pg_sleep(60)`; run
  `/top --once --kill-action cancel --filter "state='active'"
  --kill-allow-bulk`; assert the spawned session is cancelled and audit
  line is emitted to stderr.

These fixtures must follow the new "serialize catalog-mutating smoke tests"
pattern from #836.

### 5.5 Manual TUI checklist

Reviewers run a 12-step manual test before approval (paste into PR
description, tick each):

1. `/top` opens, header populated within 2 s.
2. Press `1`–`9`, each view renders without overflow at 80×24.
3. `↑/↓` selects rows; `Enter` opens drill-down; `Esc` closes.
4. `/` filter narrows results; `Esc` clears.
5. `o` opens sort picker; `O` inverts.
6. `Space` pauses; `[`/`]` rewind; header shows "PAUSED @ T-3s".
7. `k` cancels single pid (with confirmation).
8. `K` bulk modal: dry-run shows preview without firing.
9. Disconnect the DB (kill psql server briefly); header turns red,
   reconnect succeeds within 10 s.
10. `S` saves snapshot to `./rpg-top-*.json`, file is valid JSON.
11. `?` help overlay lists all keymaps.
12. `q` exits cleanly; terminal restored, no leftover alt-screen.

---

## 6. Team of Veteran Experts

The lean delivery panel for `/top`:

- **Rust + tokio engineer (lead).** Owns `mod.rs`, `state.rs`, `sampler.rs`,
  the event loop, and the WASM cfg gate. Familiar with the existing rpg
  REPL and `/ash` patterns.
- **Ratatui / TUI specialist.** Owns `renderer.rs`, all `views/*`, the
  drill-down and kill modals, theme, mouse and focus handling. Comfortable
  with crossterm event multiplexing.
- **Postgres internals SME.** Owns `sql.rs` and `views/*` SQL bodies; PG14–18
  matrix; understands `pg_stat_activity` semantics, locking, replication,
  progress views, and `pg_stat_io` (PG16+). Reviews kill semantics.
- **DBA reviewer (UX).** Sets the bar for "feels like a tool I'd use during
  an incident." Validates keymap, default sort, threshold colors, and the
  blocking tree layout.
- **QA + property-test author.** Writes `proptest` and `insta` snapshot
  suites, defines the integration matrix, owns the manual-test checklist.

Reuses the AI-panel approach codified in samospec (lead drafts; ops/security
reviewer + QA reviewer critique; convergence).

---

## 7. Sprint Plan

Eight sprints, ~1 week each. Each sprint ends with a green PR (per
`CLAUDE.md` PR workflow: CI green → REV review → squash merge).

**S1 · Scaffold & Activity view (the MVP demo).**
- Wire `/top` into the metacmd dispatcher.
- `mod.rs` lifecycle: enter alt-screen, raw mode, panic-safe restore.
- Header bar (DB summary + load + connection LED).
- `views/activity.rs` reading `pg_stat_activity` + `pg_blocking_pids`.
- Footer with hint line; `q` to quit.
- Tests: unit + first `insta` snapshot for activity view.
- **Definition of done:** a screencast showing live updates and a
  green CI on PG14–18.

**S2 · Tabs & view switching.**
- `Tabs` widget; numeric and named hotkeys.
- Add `databases`, `tables`, `indexes`, `statements`, `wal`, `functions`
  views (read-only, no drill-in yet).
- Per-view sort state.
- Tests: snapshot per view × narrow/wide.

**S3 · Filter, sort, mouse, theming.**
- `/` fuzzy filter + scoped syntax.
- `o`/`O` sort picker.
- Mouse: tab click, column-header sort, row select.
- Theme: default + dark + light + deuteranopia, truecolor detection.
- Tests: property tests on filter parser + sort.

**S4 · Drill-down overlay + AI hand-offs.**
- Q / E / A / L / W tabs.
- EXPLAIN cache; EXPLAIN ANALYZE confirmation.
- Locks pane joining `pg_locks` to `pg_class`.
- Waits sub-sampler (1 Hz) for the selected pid.
- `Shift-X` — AI eXplain on the selected pid (streams into the right half
  of the overlay; reuses `src/repl/ai_commands.rs`).
- `Shift-I` — AI Info overview of the whole `/top` view (bottom-up streaming
  panel; structured prompt with header + top-N rows + waits + blockers).
- `--ai-info` batch flag for headless mode.
- Tests: integration test with a deliberately blocked session; AI hand-off
  uses a stub provider in tests to assert prompt contents (no real API call).

**S5 · Blocking tree, replication, progress.**
- `views/blocking.rs` recursive walk, ASCII tree.
- `views/replication.rs` per-standby with bytes + time lag.
- `views/progress.rs` for the six `pg_stat_progress_*`.
- Sparklines (TPS, deadlocks, temp files) in header.
- Tests: integration test that creates a blocker chain and asserts the tree.

**S6 · Admin actions (cancel/terminate, bulk, audit).**
- `k` single, `K` bulk modal flow with two-step confirm and dry-run.
- Audit line into rpg log.
- Refuse-bulk threshold + `--kill-allow-bulk`.
- Tests: integration with deliberate `pg_sleep` victims; assert cancel
  outcome and audit emission.

**S7 · Pause/rewind, batch mode, snapshot export, config.**
- Ring buffer + pause/step.
- `--once` and `--json` headless paths.
- `S` snapshot export (json + txt).
- `.rpg.toml [top]` config plumbing + key remap.
- Tests: golden JSON for `--once`; round-trip of saved snapshot file.

**S8 · Polish, perf, docs, REV gate, release.**
- Profile: `criterion` micro-benches for sampler parse + renderer draw.
- Frame-budget guard + auto-throttle badge.
- Update `docs/COMMANDS.md`, write `docs/top.md` user guide.
- Update `CHANGELOG.md` and `Cargo.toml` per release checklist in
  `CLAUDE.md`.
- REV review pass; address blockers; squash merge.

**Critical-path risks & mitigations.**
- *Render perf at high session counts:* incremental table virtualization
  (only render visible rows). Bench in S1, fix in S8 if needed.
- *Connection contention with the rpg REPL session:* sampler always
  releases the lock between queries; admin actions take the lock once
  with a 5 s timeout.
- *PG version drift:* version-gated SQL with stub views and a single
  `pg_version_at_least(major, minor)` helper, tested in CI matrix.

---

## 8. Embedded Changelog

- **v0.2 — 2026-05-07.** Open-question review round. Resolutions:
  (1) **No system stats in v1** — header shows DB-side stats only; cpu/
  mem/io/net cells left blank with a "not collected" caption; reintroduction
  is a separate spec. (2) **`rpg top` alias** added as a clap subcommand.
  (3) **AI hand-offs accepted into S4**: `Shift-X` ("eXplain") for AI
  deep-dive on the selected pid; `Shift-I` ("Info") streams an AI overview
  of the entire `/top` view — header + sparklines + top-N rows + waits +
  blocking tree — into a bottom-up panel, with `--ai-info` for headless.
  Mnemonic: X = eXplain (matches the existing `/explain` slash-command),
  I = Info. (4) **Mouse on by default**, opt-out via `[top] mouse = false`
  or `--no-mouse`. Sprint count unchanged (8); S4 widened to include both
  AI keys; §4.10 rewritten; §4.11 added; §4.13 added.
- **v0.1 — 2026-05-07.** Initial draft authored against the rpg `main`
  branch at commit `7f93ce3`. Sourced from a comparative study of pg_top
  (gitlab.com/pg_top/pg_top) and pgcenter (github.com/lesovsky/pgcenter).
  Mirrors `/ash`'s module layout. Ratatui-first, 80×24-friendly, mouse-
  optional, kill-with-confirm. Eight-sprint plan ending with a release
  through the standard CI → REV → squash-merge flow.

---

## 9. Open questions — resolved at v0.2

1. **`/top` vs extending `/ash`.** Resolved at v0.1: separate command;
   cross-link via `Shift-A`.
2. **System stats in v1.** Resolved at v0.2: **out of scope for v1.**
   No procfs reader, no server-side helpers; header omits cpu/mem/io/net
   cleanly. Future spec to reintroduce.
3. **`rpg top` outside the REPL.** Resolved at v0.2: **yes**, ship as a
   clap subcommand alias.
4. **AI hand-off.** Resolved at v0.2: **yes, in S4**, with two keys —
   `Shift-X` (eXplain selected pid) and `Shift-I` (Info overview of the
   whole view).
5. **Mouse default.** Resolved at v0.2: **on by default**, opt-out via
   `[top] mouse = false` or `--no-mouse`.
