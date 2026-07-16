//! Data plane for `/top`. Runs the SQL strings from [`crate::top::sql`] and
//! folds their result sets into a [`Snapshot`].
//!
//! The sampler mirrors `/ash`'s observer-effect protection: it sets
//! `statement_timeout` for each query and resets it afterwards so a slow
//! `pg_stat_activity` scan fails fast rather than wedging the TUI. When a
//! tick times out the caller is told to mark the snapshot stale rather than
//! propagating the error.

use std::time::{SystemTime, UNIX_EPOCH};

use tokio_postgres::Client;

use super::sql::{ACTIVITY_SQL, SUMMARY_SQL};
use super::state::{ActivityRow, ServerSummary, Snapshot};

/// Outcome of a single sampler tick.
#[derive(Debug)]
pub enum TickResult {
    /// The tick produced a fresh snapshot.
    Ok(Box<Snapshot>),
    /// The tick was cancelled by `statement_timeout` — the TUI shows a
    /// "stale" badge and keeps the previous snapshot.
    Missed,
}

/// Run one sampler tick: server summary + active backends.
///
/// `timeout_ms` — applied per query. Pass `0` to disable.
pub async fn tick(client: &Client, timeout_ms: u64) -> anyhow::Result<TickResult> {
    let Some(server) = query_summary(client, timeout_ms).await? else {
        return Ok(TickResult::Missed);
    };
    let Some(rows) = query_activity(client, timeout_ms).await? else {
        return Ok(TickResult::Missed);
    };

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));

    let snap = Snapshot { ts, server, rows };
    Ok(TickResult::Ok(Box::new(snap)))
}

async fn query_summary(client: &Client, timeout_ms: u64) -> anyhow::Result<Option<ServerSummary>> {
    apply_timeout(client, timeout_ms).await;
    let row = match client.query_one(SUMMARY_SQL, &[]).await {
        Ok(r) => r,
        Err(e) => {
            reset_timeout(client, timeout_ms).await;
            if is_query_canceled(&e) {
                return Ok(None);
            }
            return Err(e.into());
        }
    };
    reset_timeout(client, timeout_ms).await;

    Ok(Some(ServerSummary {
        db_name: row.get::<_, String>(0),
        user: row.get::<_, String>(1),
        pg_version: row.try_get::<_, Option<String>>(2)?.unwrap_or_default(),
        uptime_secs: row.try_get::<_, Option<i64>>(3)?.unwrap_or(0),
        in_recovery: row.try_get::<_, Option<bool>>(4)?.unwrap_or(false),
        #[allow(clippy::cast_sign_loss)]
        active: row.get::<_, i32>(5).max(0) as u32,
        #[allow(clippy::cast_sign_loss)]
        idle_in_tx: row.get::<_, i32>(6).max(0) as u32,
        #[allow(clippy::cast_sign_loss)]
        waiting: row.get::<_, i32>(7).max(0) as u32,
        #[allow(clippy::cast_sign_loss)]
        total_backends: row.get::<_, i32>(8).max(0) as u32,
        #[allow(clippy::cast_sign_loss)]
        max_connections: row.get::<_, i32>(9).max(0) as u32,
        longest_xact_secs: row.try_get::<_, Option<f64>>(10)?.unwrap_or(0.0).max(0.0),
        longest_active_query_secs: row.try_get::<_, Option<f64>>(11)?.unwrap_or(0.0).max(0.0),
        deadlocks_total: row.try_get::<_, Option<i64>>(12)?.unwrap_or(0),
        temp_files_total: row.try_get::<_, Option<i64>>(13)?.unwrap_or(0),
        #[allow(clippy::cast_sign_loss)]
        autovacuum_busy: row.get::<_, i32>(14).max(0) as u32,
        #[allow(clippy::cast_sign_loss)]
        autovacuum_max: row.get::<_, i32>(15).max(0) as u32,
        #[allow(clippy::cast_sign_loss)]
        phys_slots: row.get::<_, i32>(16).max(0) as u32,
        #[allow(clippy::cast_sign_loss)]
        phys_slots_active: row.get::<_, i32>(17).max(0) as u32,
        #[allow(clippy::cast_sign_loss)]
        log_slots: row.get::<_, i32>(18).max(0) as u32,
        #[allow(clippy::cast_sign_loss)]
        log_slots_active: row.get::<_, i32>(19).max(0) as u32,
    }))
}

async fn query_activity(
    client: &Client,
    timeout_ms: u64,
) -> anyhow::Result<Option<Vec<ActivityRow>>> {
    apply_timeout(client, timeout_ms).await;
    let rows = match client.query(ACTIVITY_SQL, &[]).await {
        Ok(rs) => rs,
        Err(e) => {
            reset_timeout(client, timeout_ms).await;
            if is_query_canceled(&e) {
                return Ok(None);
            }
            return Err(e.into());
        }
    };
    reset_timeout(client, timeout_ms).await;

    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        out.push(ActivityRow {
            pid: r.get::<_, i32>(0),
            usename: r.get::<_, String>(1),
            datname: r.get::<_, String>(2),
            application_name: r.get::<_, String>(3),
            client_addr: r.get::<_, String>(4),
            backend_type: r.get::<_, String>(5),
            state: r.get::<_, String>(6),
            wait_event_type: r.get::<_, String>(7),
            wait_event: r.get::<_, String>(8),
            qtime_secs: r.try_get::<_, Option<f64>>(9)?,
            xtime_secs: r.try_get::<_, Option<f64>>(10)?,
            query: r.get::<_, String>(11),
            locks_held: r.try_get::<_, Option<i64>>(12)?.unwrap_or(0),
        });
    }
    Ok(Some(out))
}

async fn apply_timeout(client: &Client, timeout_ms: u64) {
    if timeout_ms > 0 {
        let _ = client
            .execute(&format!("set statement_timeout = '{timeout_ms}ms'"), &[])
            .await;
    }
}

async fn reset_timeout(client: &Client, timeout_ms: u64) {
    if timeout_ms > 0 {
        let _ = client.execute("set statement_timeout = 0", &[]).await;
    }
}

fn is_query_canceled(err: &tokio_postgres::Error) -> bool {
    err.code() == Some(&tokio_postgres::error::SqlState::QUERY_CANCELED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_result_ok_carries_boxed_snapshot() {
        let snap = Snapshot::default();
        let r = TickResult::Ok(Box::new(snap));
        match r {
            TickResult::Ok(b) => assert_eq!(b.ts, 0),
            TickResult::Missed => panic!("expected Ok"),
        }
    }

    #[test]
    fn tick_result_missed_signals_stale() {
        let r = TickResult::Missed;
        assert!(matches!(r, TickResult::Missed));
    }
}
