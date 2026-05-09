#!/usr/bin/env bash
set -Eeuo pipefail
IFS=$'\n\t'

# Background workload for /top demo recording.
# Spawns a steady stream of mixed backends so the activity table has
# something interesting to display: long-running queries, idle-in-tx,
# idle-with-Client-wait, lock waits, IO scans, short bursts. Self-contained:
# uses generate_series + pg_sleep + advisory locks, no schema dependencies.
#
# Usage:
#   bash demos/top-workload.sh                      # defaults
#   bash demos/top-workload.sh "host=… port=… user=… dbname=…"
#
# Per CLAUDE.md "PostgreSQL command execution" rule, every psql call uses
# PAGER=cat + --no-psqlrc + long options. Short -c is avoided.

readonly CONNINFO="${1:-host=localhost port=55433 user=postgres dbname=postgres}"

cleanup() {
  pkill -P "$$" 2>/dev/null || true
}
trap cleanup EXIT

# Wrapper that applies the project's psql conventions to every call.
run_psql() {
  env PAGER=cat psql \
    --no-psqlrc \
    --dbname="${CONNINFO}" \
    "$@"
}

run_psql_stdin() {
  env PAGER=cat psql \
    --no-psqlrc \
    --dbname="${CONNINFO}"
}

# ---------------------------------------------------------------------------
# Persistent backends — these stay connected for ~30 s so the demo always
# captures them. We feed psql via stdin (not --command) so the connection
# stays alive between commands; the wait_event will be Client:ClientRead
# while we sleep on the producer side.
# ---------------------------------------------------------------------------

# Long-lived idle backend: connects, runs a trivial select, then sits
# idle for 30 s with state='idle' and wait_event='ClientRead'.
( { echo "select 1;"; sleep 30; echo "\\q"; } \
    | run_psql_stdin >/dev/null 2>&1 ) &

# Long-lived idle-in-transaction backend: opens a tx, runs select,
# then sits with state='idle in transaction' until rollback.
( { echo "begin; select 1;"; sleep 30; echo "rollback; \\q"; } \
    | run_psql_stdin >/dev/null 2>&1 ) &

# A second idle-in-tx backend with a real lock so the locks_held column
# shows interesting numbers.
( {
    echo "begin;"
    echo "select pg_advisory_xact_lock(7);"
    echo "create temp table demo_t(x int);"
    echo "insert into demo_t select generate_series(1,1000);"
    sleep 30
    echo "rollback; \\q"
  } | run_psql_stdin >/dev/null 2>&1 ) &

# ---------------------------------------------------------------------------
# Active-query churn: spawns short-lived workers in a loop so the
# activity table is constantly rotating.
# ---------------------------------------------------------------------------
main() {
  while true; do
    # Long active query — hits qtime warn (>1s) and crit (>30s) thresholds.
    run_psql --command="select pg_sleep(10)" >/dev/null 2>&1 &

    # Medium active query — 3-5 s lifetime.
    run_psql \
      --command="select count(*) from generate_series(1, 5000000)" \
      >/dev/null 2>&1 &

    # Short bursts (Client wait events).
    for _ in $(seq 1 4); do
      run_psql --command="select 1" >/dev/null 2>&1 &
    done

    # Short idle-in-transaction (commits cleanly).
    run_psql \
      --command="begin; select pg_sleep(5); commit" \
      >/dev/null 2>&1 &

    # Lock contention via advisory locks: two backends compete on lock 42.
    run_psql --command="
      select pg_advisory_lock(42);
      select pg_sleep(2);
      select pg_advisory_unlock(42)
    " >/dev/null 2>&1 &
    run_psql --command="
      select pg_advisory_lock(42);
      select pg_sleep(0.3);
      select pg_advisory_unlock(42)
    " >/dev/null 2>&1 &

    sleep 1.2
  done
}

main "$@"
