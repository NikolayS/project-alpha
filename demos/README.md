# Demo GIFs

This directory contains recorded terminal demos of rpg features, along with
the [VHS](https://github.com/charmbracelet/vhs) tape files used to render them.

| File | What it shows |
|------|---------------|
| `gif1_optimize.gif` | Slow query → `/explain` → `/optimize` → index creation → fast re-run |
| `gif2_typo.gif` | Typo in table name → `/fix` corrects and re-executes |
| `gif3_t2s.gif` | `\t2s` text-to-SQL with confirmation, then `\yolo` auto-execute |
| `gif4_pspg.gif` | Built-in pager → `\set PAGER pspg` → same query routed through pspg |
| `gif5_lua.gif` | Custom Lua commands: `\commands`, `\slow_mean`, `\slow_total`, `\table_info` |
| `top-demo.gif` | `/top` live TUI Postgres monitor — Activity view + cursor navigation + `/top --once` headless mode |

## Prerequisites

- [VHS](https://github.com/charmbracelet/vhs) installed (`brew install vhs` on macOS)
- rpg built from source with Lua support: `cargo build --features lua`
- PostgreSQL running locally with a `demo_saas` database
- For `gif5_lua`: `pg_stat_statements` loaded via `shared_preload_libraries`

## Setting up the demo database

Create and populate the database using the provided SQL script:

```bash
createdb demo_saas
psql -d demo_saas -f demos/setup_demo_db.sql
```

See [setup_demo_db.sql](setup_demo_db.sql) for the full schema and data
generation queries.

## Rendering the GIFs

Make sure rpg is on your PATH (the tapes expect the debug build):

```bash
export PATH=/path/to/rpg/target/debug:$PATH
```

Render each GIF individually:

```bash
vhs demos/gif1_optimize.tape
vhs demos/gif2_typo.tape
vhs demos/gif3_t2s.tape
vhs demos/gif4_pspg.tape
vhs demos/gif5_lua.tape
bash demos/render-top.sh         # /top — live TUI monitor (PR #837)
```

The `/top` demo uses `demos/top-workload.sh` to spawn a steady stream of
mixed backends (active queries, idle-in-tx, advisory-lock contention).
The tape expects a local Postgres on `localhost:55433` as `postgres` —
adjust the `PG{HOST,PORT,USER,DATABASE}` env block at the top of the
tape and the `CONNINFO` argument of `top-workload.sh` for your setup.

`render-top.sh` runs vhs to produce both `.gif` and `.mp4`, then re-encodes
the gif from the mp4 through ffmpeg's palette pipeline (15 fps, bayer
dither). The native vhs gif quantizer drops frames during the busy
workload section and looks laggy; the ffmpeg pass costs ~3× file size
(11 MiB vs 3 MiB) but plays smoothly. The intermediate `top-demo.mp4` is
gitignored. The tape passes `--show-keys` so each keystroke flashes on
screen — drop the flag if you want a clean recording.

Or render all at once:

```bash
for tape in demos/*.tape; do vhs "$tape"; done
```

## Note on gif1 re-renders

`gif1_optimize.tape` creates an index on `orders (status, created_at desc)`
during the recording. Before re-rendering, drop that index so the slow-path
sequential scan is visible again:

```sql
drop index concurrently if exists orders_status_created_at_idx;
```

Then re-run the tape.

---

Copyright 2026 Postgres.ai
