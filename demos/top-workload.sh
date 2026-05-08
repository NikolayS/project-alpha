#!/usr/bin/env bash
set -Eeuo pipefail
IFS=$'\n\t'

# Background workload for /top demo recording.
# Spawns a steady stream of mixed backends so the activity table has
# something interesting to display: long-running queries, idle-in-tx,
# lock waits, IO scans, short bursts.
#
# Self-contained: uses generate_series + pg_sleep + advisory locks, no
# schema dependencies — runs against any PG instance.
#
# Usage:
#   bash demos/top-workload.sh                      # defaults
#   bash demos/top-workload.sh "host=… port=… user=… dbname=…"

CONNINFO="${1:-host=localhost port=55433 user=postgres dbname=postgres}"

cleanup() {
  pkill -P $$ 2>/dev/null || true
}
trap cleanup EXIT

while true; do
  # Long active query — visible in the qtime column for 8-12 s.
  psql "${CONNINFO}" -c "select pg_sleep(10)" >/dev/null 2>&1 &

  # Medium active query — 3-5 s lifetime.
  psql "${CONNINFO}" -c "select count(*) from generate_series(1, 5000000)" \
    >/dev/null 2>&1 &

  # Short bursts (Client wait events).
  for _ in $(seq 1 4); do
    psql "${CONNINFO}" -c "select 1" >/dev/null 2>&1 &
  done

  # Idle-in-transaction backend.
  psql "${CONNINFO}" -c "begin; select pg_sleep(6); commit" \
    >/dev/null 2>&1 &

  # Lock contention via advisory locks: two backends compete on lock 42.
  psql "${CONNINFO}" -c "
    select pg_advisory_lock(42);
    select pg_sleep(2);
    select pg_advisory_unlock(42)
  " >/dev/null 2>&1 &
  psql "${CONNINFO}" -c "
    select pg_advisory_lock(42);
    select pg_sleep(0.3);
    select pg_advisory_unlock(42)
  " >/dev/null 2>&1 &

  sleep 1.2
done
