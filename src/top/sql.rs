//! SQL strings for `/top`. Co-located so each view's query is easy to find,
//! review, and version-gate. All queries follow the project's lower-case-
//! keyword style (`CLAUDE.md` SQL style guide).
//!
//! S1 ships two queries: a server-summary header read and the Activity
//! view body. Later sprints add one query per additional view; the version
//! gating helper [`SUPPORTED_PG_VERSION_HINT`] is here so they can opt in
//! per major release.

/// One-shot server summary used in the header bar.
///
/// Returns a single row with: db name, current user, server version short
/// string, uptime in seconds, recovery flag, and aggregate session counts
/// from `pg_stat_activity`. Excludes the rpg backend itself so the active
/// count is not inflated by the monitor.
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
        count(*)::int                                               as total_backends
    from pg_stat_activity
    where pid <> pg_backend_pid()
";

/// Activity view body — one row per non-rpg backend, ordered with active
/// backends first and the longest-running queries on top.
///
/// The query is intentionally portable across PG14–PG18: every column used
/// here exists in PG14's `pg_stat_activity`. Newer columns (e.g.
/// `query_id` from PG14, `leader_pid`) are excluded to keep S1 minimal.
pub const ACTIVITY_SQL: &str = "
    select
        pid,
        coalesce(usename, '')                                       as usename,
        coalesce(datname, '')                                       as datname,
        coalesce(application_name, '')                              as application_name,
        coalesce(client_addr::text, '')                             as client_addr,
        coalesce(backend_type, '')                                  as backend_type,
        coalesce(state, '')                                         as state,
        coalesce(wait_event_type, '')                               as wait_event_type,
        coalesce(wait_event, '')                                    as wait_event,
        case
            when query_start is null then null
            else extract(epoch from (now() - query_start))::float8
        end                                                         as qtime_secs,
        case
            when xact_start is null then null
            else extract(epoch from (now() - xact_start))::float8
        end                                                         as xtime_secs,
        coalesce(left(query, 500), '')                              as query
    from pg_stat_activity
    where pid <> pg_backend_pid()
    order by
        case state when 'active' then 0 else 1 end,
        qtime_secs desc nulls last,
        pid
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_sql_mentions_required_columns() {
        // Spot-check for the columns the renderer will read by index.
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
        ] {
            assert!(
                SUMMARY_SQL.contains(col),
                "summary SQL is missing column {col}"
            );
        }
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
        assert!(ACTIVITY_SQL.contains("case state when 'active' then 0 else 1 end"));
        assert!(ACTIVITY_SQL.contains("qtime_secs desc nulls last"));
    }

    /// Regression test: tokio-postgres cannot deserialize Postgres `numeric`
    /// directly into Rust `f64`; we must cast `extract(epoch from …)` to
    /// `float8` explicitly. Surfaced in S1 manual testing as
    /// "error deserializing column 9".
    #[test]
    fn elapsed_time_columns_cast_to_float8() {
        assert!(
            ACTIVITY_SQL.contains("(now() - query_start))::float8"),
            "qtime_secs must be cast to float8"
        );
        assert!(
            ACTIVITY_SQL.contains("(now() - xact_start))::float8"),
            "xtime_secs must be cast to float8"
        );
    }
}
