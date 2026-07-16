//! SQL strings for `/top`. Co-located so each view's query is easy to find,
//! review, and version-gate. All queries follow the project's lower-case-
//! keyword style (`CLAUDE.md` SQL style guide).
//!
//! S1 ships two queries: a server-summary header read and the Activity
//! view body. Later sprints add one query per additional view; views that
//! need columns gated on PG ≥ 16 should branch at construction time using
//! the server's `server_version_num` once the sampler exposes it.

/// Server summary used in the header bar.
///
/// Returns a single row mixing per-cluster facts (uptime, recovery,
/// `max_connections`), per-database aggregates (`deadlocks`, `temp_files` —
/// summed across every database the cluster knows about), and per-session
/// aggregates from `pg_stat_activity` (active / idle-in-tx / wait counts,
/// longest active transaction, longest active query). Excludes the rpg
/// monitor backend itself so the active count is not inflated.
pub const SUMMARY_SQL: &str = r"
    select
        current_database()                                          as db_name,
        current_user                                                as usename,
        substring(version() from '[0-9]+\.[0-9]+')                  as pg_version,
        extract(
            epoch from (now() - pg_postmaster_start_time())
        )::bigint                                                   as uptime_secs,
        pg_is_in_recovery()                                         as in_recovery,
        coalesce(sum(case when state = 'active' then 1 else 0 end), 0)::int
                                                                    as active,
        coalesce(sum(case
            when state in ('idle in transaction',
                           'idle in transaction (aborted)') then 1 else 0 end), 0)::int
                                                                    as idle_in_tx,
        coalesce(sum(case
            when wait_event is not null and state = 'active' then 1 else 0 end), 0)::int
                                                                    as waiting,
        count(*)::int                                               as total_backends,
        current_setting('max_connections')::int                     as max_connections,
        coalesce(
            extract(epoch from (now() - min(xact_start)))::float8,
            0::float8
        )                                                           as longest_xact_secs,
        coalesce(
            extract(epoch from (
                now() - min(query_start) filter (where state = 'active')
            ))::float8,
            0::float8
        )                                                           as longest_active_query_secs,
        (select coalesce(sum(deadlocks), 0)::int8 from pg_stat_database)
                                                                    as deadlocks_total,
        (select coalesce(sum(temp_files), 0)::int8 from pg_stat_database)
                                                                    as temp_files_total,
        coalesce(
            sum(case when backend_type = 'autovacuum worker' then 1 else 0 end),
            0
        )::int                                                      as autovacuum_busy,
        current_setting('autovacuum_max_workers')::int              as autovacuum_max,
        (select coalesce(count(*), 0)::int from pg_replication_slots
            where slot_type = 'physical')                           as phys_slots,
        (select coalesce(count(*) filter (where active), 0)::int
            from pg_replication_slots where slot_type = 'physical') as phys_slots_active,
        (select coalesce(count(*), 0)::int from pg_replication_slots
            where slot_type = 'logical')                            as log_slots,
        (select coalesce(count(*) filter (where active), 0)::int
            from pg_replication_slots where slot_type = 'logical')  as log_slots_active
    from pg_stat_activity
    where pid <> pg_backend_pid()
";

/// Activity view body — one row per non-rpg backend, with a count of
/// granted locks held per pid (left-joined from `pg_locks`). Ordered with
/// active backends first and longest-running queries on top.
///
/// Portable across PG14–PG18: every column used here exists in PG14's
/// `pg_stat_activity`. Columns available in PG14 but excluded to keep S1
/// minimal: `query_id`, `leader_pid`. They will be added when the
/// drill-down overlay (S4) needs them.
pub const ACTIVITY_SQL: &str = "
    with locks as (
        select pid, count(*)::bigint as n
        from pg_locks
        where granted
        group by pid
    )
    select
        a.pid,
        coalesce(a.usename, '')                                     as usename,
        coalesce(a.datname, '')                                     as datname,
        coalesce(a.application_name, '')                            as application_name,
        coalesce(a.client_addr::text, '')                           as client_addr,
        coalesce(a.backend_type, '')                                as backend_type,
        coalesce(a.state, '')                                       as state,
        coalesce(a.wait_event_type, '')                             as wait_event_type,
        coalesce(a.wait_event, '')                                  as wait_event,
        case
            when a.query_start is null then null
            else extract(epoch from (now() - a.query_start))::float8
        end                                                         as qtime_secs,
        case
            when a.xact_start is null then null
            else extract(epoch from (now() - a.xact_start))::float8
        end                                                         as xtime_secs,
        coalesce(left(a.query, 500), '')                            as query,
        coalesce(l.n, 0)::bigint                                    as locks_held
    from pg_stat_activity as a
    left join locks as l using (pid)
    where a.pid <> pg_backend_pid()
    order by
        case a.state when 'active' then 0 else 1 end,
        qtime_secs desc nulls last,
        a.pid
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_sql_mentions_required_columns() {
        for col in [
            "db_name",
            "usename",
            "pg_version",
            "uptime_secs",
            "in_recovery",
            "active",
            "idle_in_tx",
            "waiting",
            "total_backends",
            "max_connections",
            "longest_xact_secs",
            "longest_active_query_secs",
            "deadlocks_total",
            "temp_files_total",
            "autovacuum_busy",
            "autovacuum_max",
            "phys_slots",
            "phys_slots_active",
            "log_slots",
            "log_slots_active",
        ] {
            assert!(
                SUMMARY_SQL.contains(col),
                "summary SQL is missing column {col}"
            );
        }
    }

    #[test]
    fn summary_sql_excludes_self_backend() {
        assert!(
            SUMMARY_SQL.contains("pid <> pg_backend_pid()"),
            "summary SQL must exclude the rpg monitor backend itself",
        );
    }

    #[test]
    fn activity_sql_excludes_self_backend() {
        assert!(
            ACTIVITY_SQL.contains("pid <> pg_backend_pid()"),
            "activity SQL must exclude the rpg monitor backend itself"
        );
    }

    #[test]
    fn activity_sql_orders_active_first_then_qtime_desc() {
        assert!(ACTIVITY_SQL.contains("case a.state when 'active' then 0 else 1 end"));
        assert!(ACTIVITY_SQL.contains("qtime_secs desc nulls last"));
    }

    #[test]
    fn activity_sql_includes_locks_held_column() {
        assert!(
            ACTIVITY_SQL.contains("as locks_held"),
            "activity SQL must include the locks_held column",
        );
        assert!(
            ACTIVITY_SQL.contains("from pg_locks"),
            "locks_held must come from pg_locks",
        );
        assert!(
            ACTIVITY_SQL.contains("where granted"),
            "only granted locks should be counted",
        );
    }

    /// Regression test for the deserialization bug surfaced during S1
    /// manual testing: tokio-postgres cannot decode Postgres `numeric`
    /// directly into Rust `f64`; the cast must be `::float8`.
    #[test]
    fn elapsed_time_columns_cast_to_float8() {
        assert!(
            ACTIVITY_SQL.contains("(now() - a.query_start))::float8"),
            "qtime_secs must be cast to float8"
        );
        assert!(
            ACTIVITY_SQL.contains("(now() - a.xact_start))::float8"),
            "xtime_secs must be cast to float8"
        );
    }
}
