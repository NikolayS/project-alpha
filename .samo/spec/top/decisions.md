# /top — decisions log

samospec convention: every accepted/rejected/deferred design choice is captured
here so future reviewers can see *why*, not only *what*.

## v0.2 — open-question review (2026-05-07)

### Accepted (this round)

- **System stats out of v1.** No procfs reader and no server-side helpers
  in v1 of `/top`. Header omits cpu/mem/io/net cells with a one-line
  caption. Reintroduction lives in a separate spec
  (`.samo/spec/top-system-stats/`, future). `Snapshot` keeps the optional
  fields so adding it later is additive.
- **`rpg top` clap subcommand alias.** Bare `rpg top [args...]` aliases
  to `rpg --command "/top args..."`. Mirrors `psql -c "\l"` muscle memory
  and removes a quoting layer.
- **Two AI hand-off keys, both in S4.** Mnemonic split: `X` = eXplain
  (matches the existing `/explain` slash-command), `I` = Info.
  - `Shift-X` — eXplain the selected pid (sends row + cached EXPLAIN
    through `/explain`, streams into the drill-down overlay).
  - `Shift-I` — Info overview of the whole view (packages header,
    sparklines, top-N rows, waits, blocking tree; streams to a bottom-up
    panel; has `--ai-info` for headless mode).
  Both reuse `src/repl/ai_commands.rs` plumbing; no new AI surface.
- **Mouse on by default.** `crossterm::EnableMouseCapture` is set on
  alt-screen entry. Opt-out via `[top] mouse = false` or `--no-mouse`
  (useful for native terminal copy/paste).

### Implications for sprint plan

- S4 grows to absorb both AI keys (still bounded by stub-provider tests,
  no real API calls in CI).
- S5 no longer carries any procfs / system-stats work — pure DB-side
  features (replication, progress, sparklines).
- No new dependencies needed. Ratatui 0.30 and crossterm already cover
  mouse capture; AI plumbing already in tree.

---

## v0.1 — initial draft (2026-05-07)

### Accepted

- **Single new command, `/top`, in the `/` namespace.** Per `docs/COMMANDS.md`,
  any rpg-specific extension uses `/`. Adding `/top` (vs hijacking the existing
  psql-style `\watch`) keeps the namespace policy intact.
- **Mirror `/ash` module layout.** New code under `src/top/{mod,renderer,
  sampler,state}.rs` so reviewers can pattern-match. Reuse `terminal_has_
  truecolor()` and the small-terminal stub from `src/ash/renderer.rs`.
- **Ratatui 0.30** (already in `Cargo.toml`). No new heavyweight dep.
- **Native-only.** `#[cfg(not(target_arch = "wasm32"))]` gate, mirroring
  `/ash` and `/rpg` in `src/repl/ai_commands.rs`.
- **Read-only by default.** Cancel/terminate require an explicit modal
  confirm; bulk &gt;200 needs `--kill-allow-bulk`.
- **Eight-sprint plan.** S1 ships an MVP demo (activity view + header)
  behind CI-green + REV review; later sprints layer features.

### Deferred

- **Server-side OS-stats helper functions** (`rpg.system_stats()`). Out of
  scope for v1; v0.2 confirms system stats overall are deferred to a
  separate future spec. v1 of `/top` shows DB-side stats only.
- **`/profile` (wait-event sampler) and `/record`/`/report` (offline
  recordings).** These are separate features identified during the same
  pg_top + pgcenter study; their specs will live alongside this one under
  `.samo/spec/{profile,record,report}/`.
- **`pg_proctab`-based remote OS stats** (pg_top's mechanism). Not adopted;
  we prefer rpg-native helper functions when extension installation is
  acceptable, and procfs when local.
- **AI hand-off (`Shift-I` send selected query to `/explain`).** Listed in
  Open Questions; deferred to S4 or later depending on review.

### Rejected

- **Extending `/ash` instead of a new command.** `/ash` is a *history* view;
  `/top` is a *now* view. Conflating them confuses semantics and key bindings.
  Cross-link with `Shift-A` instead.
- **Pure-SQL recursive blocking tree.** Doable via CTE on `pg_blocking_pids`
  but harder to enrich with query summaries and wait events. Rust-side
  walk picked for clarity; SQL fallback can be added later if needed.
- **Hard requirement on `pg_stat_statements`.** Optional — the Statements
  view shows a clear "extension not installed" stub instead of breaking.

### Open

See §9 of `SPEC.md` ("Open questions for the review round"). These are
expected to converge by v0.2 of this spec.
