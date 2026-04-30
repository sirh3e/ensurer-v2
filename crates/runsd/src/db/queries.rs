//! Runtime-checked sqlx queries (no DATABASE_URL required at compile time).
use sqlx::{Pool, Sqlite};

use common::{
    model::{CalcStatus, Calculation, Run, RunStatus},
    types::{CalcId, RunId},
};

use super::row::{CalcRow, EventRow, RowConversionError, RunRow, now_millis};
use crate::error::AppError;

pub type Db = Pool<Sqlite>;

// ── Runs ──────────────────────────────────────────────────────────────────────

pub async fn insert_run(
    db: &Db,
    id: &RunId,
    jira_issue_id: &str,
    submitted_by: &str,
) -> Result<(), AppError> {
    let now = now_millis();
    sqlx::query(
        "INSERT INTO runs (id, jira_issue_id, submitted_by, status, created_at, updated_at)
         VALUES (?, ?, ?, 'pending', ?, ?)",
    )
    .bind(id.0.to_string())
    .bind(jira_issue_id)
    .bind(submitted_by)
    .bind(now)
    .bind(now)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn update_run_status(db: &Db, id: &RunId, status: RunStatus) -> Result<(), AppError> {
    let now = now_millis();
    sqlx::query("UPDATE runs SET status = ?, updated_at = ? WHERE id = ?")
        .bind(status.to_string())
        .bind(now)
        .bind(id.0.to_string())
        .execute(db)
        .await?;
    Ok(())
}

pub async fn get_run(db: &Db, id: &RunId) -> Result<Option<Run>, AppError> {
    let row: Option<RunRow> = sqlx::query_as(
        "SELECT id, jira_issue_id, submitted_by, status, created_at, updated_at
         FROM runs WHERE id = ?",
    )
    .bind(id.0.to_string())
    .fetch_optional(db)
    .await?;

    match row {
        None => Ok(None),
        Some(r) => {
            let calcs = list_calculations_for_run(db, id).await?;
            Ok(Some(r.try_into_run(calcs)?))
        }
    }
}

pub async fn list_runs(
    db: &Db,
    status_filter: Option<&str>,
    limit: u32,
    cursor_created_at: Option<i64>,
    cursor_id: Option<&str>,
) -> Result<Vec<Run>, AppError> {
    let rows: Vec<RunRow> = match (status_filter, cursor_created_at, cursor_id) {
        (Some(s), Some(cat), Some(cid)) => {
            sqlx::query_as(
                "SELECT id, jira_issue_id, submitted_by, status, created_at, updated_at
             FROM runs WHERE status = ? AND (created_at < ? OR (created_at = ? AND id < ?))
             ORDER BY created_at DESC, id DESC LIMIT ?",
            )
            .bind(s)
            .bind(cat)
            .bind(cat)
            .bind(cid)
            .bind(limit as i64)
            .fetch_all(db)
            .await?
        }
        (Some(s), None, None) => {
            sqlx::query_as(
                "SELECT id, jira_issue_id, submitted_by, status, created_at, updated_at
             FROM runs WHERE status = ? ORDER BY created_at DESC, id DESC LIMIT ?",
            )
            .bind(s)
            .bind(limit as i64)
            .fetch_all(db)
            .await?
        }
        (None, Some(cat), Some(cid)) => {
            sqlx::query_as(
                "SELECT id, jira_issue_id, submitted_by, status, created_at, updated_at
             FROM runs WHERE (created_at < ? OR (created_at = ? AND id < ?))
             ORDER BY created_at DESC, id DESC LIMIT ?",
            )
            .bind(cat)
            .bind(cat)
            .bind(cid)
            .bind(limit as i64)
            .fetch_all(db)
            .await?
        }
        _ => {
            sqlx::query_as(
                "SELECT id, jira_issue_id, submitted_by, status, created_at, updated_at
             FROM runs ORDER BY created_at DESC, id DESC LIMIT ?",
            )
            .bind(limit as i64)
            .fetch_all(db)
            .await?
        }
    };

    let mut runs = Vec::with_capacity(rows.len());
    for row in rows {
        let run_id = row.parse_id()?;
        let calcs = list_calculations_for_run(db, &run_id).await?;
        runs.push(row.try_into_run(calcs)?);
    }
    Ok(runs)
}

// ── Calculations ──────────────────────────────────────────────────────────────

pub async fn insert_calculation(db: &Db, calc: &Calculation) -> Result<(), AppError> {
    let now = now_millis();
    let input_str = serde_json::to_string(&calc.input_json)?;
    sqlx::query(
        "INSERT INTO calculations
         (id, run_id, kind, input_json, idempotency_key, status, attempt, max_attempts,
          created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(calc.id.0.to_string())
    .bind(calc.run_id.0.to_string())
    .bind(&calc.kind)
    .bind(input_str)
    .bind(&calc.idempotency_key)
    .bind(calc.status.to_string())
    .bind(calc.attempt as i64)
    .bind(calc.max_attempts as i64)
    .bind(now)
    .bind(now)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn get_calculation(db: &Db, id: &CalcId) -> Result<Option<Calculation>, AppError> {
    let row: Option<CalcRow> = sqlx::query_as(
        "SELECT id, run_id, kind, input_json, idempotency_key, status, attempt, max_attempts,
                next_attempt_at, lease_owner, lease_expires_at, error_kind, error_message,
                result_path, created_at, started_at, completed_at, updated_at
         FROM calculations WHERE id = ?",
    )
    .bind(id.0.to_string())
    .fetch_optional(db)
    .await?;
    row.map(|r| r.try_into_calc())
        .transpose()
        .map_err(AppError::from)
}

pub async fn list_calculations_for_run(
    db: &Db,
    run_id: &RunId,
) -> Result<Vec<Calculation>, AppError> {
    let rows: Vec<CalcRow> = sqlx::query_as(
        "SELECT id, run_id, kind, input_json, idempotency_key, status, attempt, max_attempts,
                next_attempt_at, lease_owner, lease_expires_at, error_kind, error_message,
                result_path, created_at, started_at, completed_at, updated_at
         FROM calculations WHERE run_id = ? ORDER BY created_at ASC",
    )
    .bind(run_id.0.to_string())
    .fetch_all(db)
    .await?;
    rows.into_iter()
        .map(|r| r.try_into_calc())
        .collect::<Result<Vec<_>, RowConversionError>>()
        .map_err(AppError::from)
}

pub async fn update_calc_started(
    db: &Db,
    id: &CalcId,
    lease_owner: &str,
    lease_expires_at: i64,
) -> Result<(), AppError> {
    let now = now_millis();
    sqlx::query(
        "UPDATE calculations
         SET status = 'running', started_at = COALESCE(started_at, ?),
             lease_owner = ?, lease_expires_at = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(now)
    .bind(lease_owner)
    .bind(lease_expires_at)
    .bind(now)
    .bind(id.0.to_string())
    .execute(db)
    .await?;
    Ok(())
}

pub async fn update_calc_heartbeat(
    db: &Db,
    id: &CalcId,
    lease_expires_at: i64,
) -> Result<(), AppError> {
    let now = now_millis();
    sqlx::query("UPDATE calculations SET lease_expires_at = ?, updated_at = ? WHERE id = ?")
        .bind(lease_expires_at)
        .bind(now)
        .bind(id.0.to_string())
        .execute(db)
        .await?;
    Ok(())
}

pub async fn update_calc_succeeded(
    db: &Db,
    id: &CalcId,
    result_path: &str,
) -> Result<(), AppError> {
    let now = now_millis();
    sqlx::query(
        "UPDATE calculations
         SET status = 'succeeded', completed_at = ?, result_path = ?,
             lease_owner = NULL, lease_expires_at = NULL, updated_at = ?
         WHERE id = ?",
    )
    .bind(now)
    .bind(result_path)
    .bind(now)
    .bind(id.0.to_string())
    .execute(db)
    .await?;
    Ok(())
}

pub async fn update_calc_failed(
    db: &Db,
    id: &CalcId,
    error_kind: &str,
    error_message: &str,
) -> Result<(), AppError> {
    let now = now_millis();
    sqlx::query(
        "UPDATE calculations
         SET status = 'failed', completed_at = ?, error_kind = ?, error_message = ?,
             lease_owner = NULL, lease_expires_at = NULL, updated_at = ?
         WHERE id = ?",
    )
    .bind(now)
    .bind(error_kind)
    .bind(error_message)
    .bind(now)
    .bind(id.0.to_string())
    .execute(db)
    .await?;
    Ok(())
}

pub async fn update_calc_retrying(
    db: &Db,
    id: &CalcId,
    attempt: u32,
    next_attempt_at: i64,
) -> Result<(), AppError> {
    let now = now_millis();
    sqlx::query(
        "UPDATE calculations
         SET status = 'retrying', attempt = ?, next_attempt_at = ?,
             lease_owner = NULL, lease_expires_at = NULL, updated_at = ?
         WHERE id = ?",
    )
    .bind(attempt as i64)
    .bind(next_attempt_at)
    .bind(now)
    .bind(id.0.to_string())
    .execute(db)
    .await?;
    Ok(())
}

pub async fn update_calc_cancelled(db: &Db, id: &CalcId) -> Result<(), AppError> {
    let now = now_millis();
    sqlx::query(
        "UPDATE calculations
         SET status = 'cancelled', completed_at = ?,
             lease_owner = NULL, lease_expires_at = NULL, updated_at = ?
         WHERE id = ?",
    )
    .bind(now)
    .bind(now)
    .bind(id.0.to_string())
    .execute(db)
    .await?;
    Ok(())
}

pub async fn update_calc_pending(db: &Db, id: &CalcId) -> Result<(), AppError> {
    let now = now_millis();
    sqlx::query(
        "UPDATE calculations
         SET status = 'pending', attempt = 0, next_attempt_at = NULL,
             error_kind = NULL, error_message = NULL,
             lease_owner = NULL, lease_expires_at = NULL, updated_at = ?
         WHERE id = ?",
    )
    .bind(now)
    .bind(id.0.to_string())
    .execute(db)
    .await?;
    Ok(())
}

pub async fn list_expired_leases(db: &Db, now: i64) -> Result<Vec<Calculation>, AppError> {
    let rows: Vec<CalcRow> = sqlx::query_as(
        "SELECT id, run_id, kind, input_json, idempotency_key, status, attempt, max_attempts,
                next_attempt_at, lease_owner, lease_expires_at, error_kind, error_message,
                result_path, created_at, started_at, completed_at, updated_at
         FROM calculations
         WHERE status = 'running' AND lease_expires_at IS NOT NULL AND lease_expires_at < ?",
    )
    .bind(now)
    .fetch_all(db)
    .await?;
    rows.into_iter()
        .map(|r| r.try_into_calc())
        .collect::<Result<Vec<_>, RowConversionError>>()
        .map_err(AppError::from)
}

pub async fn list_ready_retries(db: &Db, now: i64) -> Result<Vec<Calculation>, AppError> {
    let rows: Vec<CalcRow> = sqlx::query_as(
        "SELECT id, run_id, kind, input_json, idempotency_key, status, attempt, max_attempts,
                next_attempt_at, lease_owner, lease_expires_at, error_kind, error_message,
                result_path, created_at, started_at, completed_at, updated_at
         FROM calculations
         WHERE status = 'retrying' AND next_attempt_at IS NOT NULL AND next_attempt_at <= ?",
    )
    .bind(now)
    .fetch_all(db)
    .await?;
    rows.into_iter()
        .map(|r| r.try_into_calc())
        .collect::<Result<Vec<_>, RowConversionError>>()
        .map_err(AppError::from)
}

pub async fn list_active_run_ids(db: &Db) -> Result<Vec<RunId>, AppError> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT run_id FROM calculations
         WHERE status NOT IN ('succeeded','failed','cancelled')",
    )
    .fetch_all(db)
    .await?;
    rows.into_iter()
        .map(|(id,)| {
            id.parse().map(RunId).map_err(|e| {
                AppError::RowConversion(RowConversionError::InvalidUuid {
                    column: "calculations.run_id",
                    source: e,
                })
            })
        })
        .collect()
}

// ── Events ────────────────────────────────────────────────────────────────────

pub async fn insert_event(
    db: &Db,
    run_id: Option<&str>,
    calculation_id: Option<&str>,
    kind: &str,
    payload_json: &str,
) -> Result<i64, AppError> {
    let now = now_millis();
    let result = sqlx::query(
        "INSERT INTO events (run_id, calculation_id, kind, payload_json, created_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(run_id)
    .bind(calculation_id)
    .bind(kind)
    .bind(payload_json)
    .bind(now)
    .execute(db)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn list_events_since(
    db: &Db,
    since_seq: i64,
    limit: i64,
) -> Result<Vec<EventRow>, AppError> {
    let rows: Vec<EventRow> = sqlx::query_as(
        "SELECT seq, run_id, calculation_id, kind, payload_json, created_at
         FROM events WHERE seq > ? ORDER BY seq ASC LIMIT ?",
    )
    .bind(since_seq)
    .bind(limit)
    .fetch_all(db)
    .await?;
    Ok(rows)
}

pub async fn list_events_for_run(
    db: &Db,
    run_id: &RunId,
    since_seq: i64,
) -> Result<Vec<EventRow>, AppError> {
    let rows: Vec<EventRow> = sqlx::query_as(
        "SELECT seq, run_id, calculation_id, kind, payload_json, created_at
         FROM events WHERE run_id = ? AND seq > ? ORDER BY seq ASC",
    )
    .bind(run_id.0.to_string())
    .bind(since_seq)
    .fetch_all(db)
    .await?;
    Ok(rows)
}

// ── Crash recovery ────────────────────────────────────────────────────────────

pub async fn crash_recovery_sweep(
    db: &Db,
    now: i64,
    _max_attempts_default: u32,
) -> Result<(), AppError> {
    let rows: Vec<CalcRow> = sqlx::query_as(
        "SELECT id, run_id, kind, input_json, idempotency_key, status, attempt, max_attempts,
                next_attempt_at, lease_owner, lease_expires_at, error_kind, error_message,
                result_path, created_at, started_at, completed_at, updated_at
         FROM calculations
         WHERE status IN ('running','retrying')
           AND (lease_expires_at IS NULL OR lease_expires_at < ?)",
    )
    .bind(now)
    .fetch_all(db)
    .await?;

    for row in rows {
        let id_str = row.id.clone();
        let calc = row.try_into_calc()?;
        let next_attempt = calc.attempt + 1;
        let max = calc.max_attempts;

        if next_attempt > max {
            sqlx::query(
                "UPDATE calculations SET status = 'failed', error_kind = 'crash_exhausted',
                 lease_owner = NULL, lease_expires_at = NULL, completed_at = ?, updated_at = ?
                 WHERE id = ?",
            )
            .bind(now)
            .bind(now)
            .bind(id_str)
            .execute(db)
            .await?;
        } else {
            let next_at = now + (next_attempt as i64 * 5_000);
            sqlx::query(
                "UPDATE calculations SET status = 'retrying', attempt = ?,
                 next_attempt_at = ?, lease_owner = NULL, lease_expires_at = NULL, updated_at = ?
                 WHERE id = ?",
            )
            .bind(next_attempt as i64)
            .bind(next_at)
            .bind(now)
            .bind(id_str)
            .execute(db)
            .await?;
        }
    }
    Ok(())
}

pub async fn prune_old_events(db: &Db, older_than_ms: i64) -> Result<u64, AppError> {
    let result = sqlx::query("DELETE FROM events WHERE created_at < ?")
        .bind(older_than_ms)
        .execute(db)
        .await?;
    Ok(result.rows_affected())
}

pub async fn get_calc_statuses_for_run(
    db: &Db,
    run_id: &RunId,
) -> Result<Vec<CalcStatus>, AppError> {
    let rows: Vec<(String,)> = sqlx::query_as("SELECT status FROM calculations WHERE run_id = ?")
        .bind(run_id.0.to_string())
        .fetch_all(db)
        .await?;
    rows.into_iter()
        .map(|(s,)| {
            s.parse::<CalcStatus>().map_err(|_| {
                AppError::RowConversion(RowConversionError::UnknownVariant {
                    column: "calculations.status",
                    value: s,
                })
            })
        })
        .collect()
}
