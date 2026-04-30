use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;

use common::{
    model::{ListRunsResponse, SubmitRunRequest, SubmitRunResponse},
    types::{CalcId, RunId},
};

use crate::{api::state::AppState, error::AppError};

// ── Healthz ───────────────────────────────────────────────────────────────────

pub async fn healthz(State(state): State<AppState>) -> impl IntoResponse {
    let db_ok = sqlx::query("SELECT 1")
        .execute(&state.read_pool)
        .await
        .is_ok();
    let queue_depth: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM calculations WHERE status IN ('pending','retrying')",
    )
    .fetch_one(&state.read_pool)
    .await
    .unwrap_or(0);
    let status = if db_ok { "ok" } else { "degraded" };
    Json(serde_json::json!({
        "status": status,
        "db": if db_ok { "ok" } else { "error" },
        "queue_depth": queue_depth,
    }))
}

// ── Metrics (Prometheus text format) ─────────────────────────────────────────

pub async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    let run_counts: Vec<(String, i64)> =
        sqlx::query_as("SELECT status, COUNT(*) FROM runs GROUP BY status")
            .fetch_all(&state.read_pool)
            .await
            .unwrap_or_default();

    let calc_counts: Vec<(String, i64)> =
        sqlx::query_as("SELECT status, COUNT(*) FROM calculations GROUP BY status")
            .fetch_all(&state.read_pool)
            .await
            .unwrap_or_default();

    let active_leases: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM calculations WHERE status = 'running' AND lease_expires_at IS NOT NULL",
    )
    .fetch_one(&state.read_pool)
    .await
    .unwrap_or(0);

    let mut out = String::new();
    out.push_str("# HELP runsd_runs_total Runs by status\n# TYPE runsd_runs_total gauge\n");
    for (status, count) in &run_counts {
        out.push_str(&format!(
            "runsd_runs_total{{status=\"{status}\"}} {count}\n"
        ));
    }
    out.push_str(
        "# HELP runsd_calculations_total Calculations by status\n# TYPE runsd_calculations_total gauge\n",
    );
    for (status, count) in &calc_counts {
        out.push_str(&format!(
            "runsd_calculations_total{{status=\"{status}\"}} {count}\n"
        ));
    }
    out.push_str(
        "# HELP runsd_active_leases Active calculation leases\n# TYPE runsd_active_leases gauge\n",
    );
    out.push_str(&format!("runsd_active_leases {active_leases}\n"));

    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        out,
    )
}

// ── Runs ──────────────────────────────────────────────────────────────────────

pub async fn submit_run(
    State(state): State<AppState>,
    Json(body): Json<SubmitRunRequest>,
) -> Result<impl IntoResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    let run_id = state
        .supervisor
        .submit_run(body.jira_issue_id, "anonymous".into(), body.calculations)
        .await?;
    Ok((StatusCode::CREATED, Json(SubmitRunResponse { run_id })))
}

#[derive(Debug, Deserialize)]
pub struct ListRunsParams {
    pub status: Option<String>,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

pub async fn list_runs(
    State(state): State<AppState>,
    Query(params): Query<ListRunsParams>,
) -> Result<impl IntoResponse, AppError> {
    let limit = params.limit.unwrap_or(20).min(100);

    // cursor = "<created_at>,<id>"
    let (cursor_cat, cursor_id) = match params.cursor.as_deref() {
        None => (None, None),
        Some(c) => {
            let mut parts = c.splitn(2, ',');
            let cat = parts.next().and_then(|s| s.parse::<i64>().ok());
            let id = parts.next().map(|s| s.to_string());
            (cat, id)
        }
    };

    let runs = state
        .db
        .list_runs(params.status, limit, cursor_cat, cursor_id)
        .await?;

    let next_cursor = if runs.len() == limit as usize {
        runs.last()
            .map(|r| format!("{},{}", r.created_at.timestamp_millis(), r.id))
    } else {
        None
    };

    Ok(Json(ListRunsResponse { runs, next_cursor }))
}

pub async fn get_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let run_id = RunId(
        id.parse()
            .map_err(|_| AppError::BadRequest("invalid run id".into()))?,
    );
    let run = state.db.get_run(run_id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(run))
}

pub async fn cancel_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let run_id = RunId(
        id.parse()
            .map_err(|_| AppError::BadRequest("invalid run id".into()))?,
    );
    state.supervisor.cancel_run(run_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Calculations ──────────────────────────────────────────────────────────────

pub async fn get_calculation(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let calc_id = CalcId(
        id.parse()
            .map_err(|_| AppError::BadRequest("invalid calc id".into()))?,
    );
    let calc = state
        .db
        .get_calculation(calc_id)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(calc))
}

pub async fn retry_calculation(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let calc_id = CalcId(
        id.parse()
            .map_err(|_| AppError::BadRequest("invalid calc id".into()))?,
    );
    let calc = state
        .db
        .get_calculation(calc_id.clone())
        .await?
        .ok_or(AppError::NotFound)?;

    if calc.status != common::model::CalcStatus::Failed {
        return Err(AppError::Conflict(
            "only calculations in 'failed' status can be retried".into(),
        ));
    }

    state.supervisor.retry_calc(calc.run_id, calc_id).await?;
    Ok(StatusCode::ACCEPTED)
}

pub async fn cancel_calculation(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let calc_id = CalcId(
        id.parse()
            .map_err(|_| AppError::BadRequest("invalid calc id".into()))?,
    );
    let calc = state
        .db
        .get_calculation(calc_id.clone())
        .await?
        .ok_or(AppError::NotFound)?;

    state.supervisor.cancel_calc(calc.run_id, calc_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_result(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let calc_id = CalcId(
        id.parse()
            .map_err(|_| AppError::BadRequest("invalid calc id".into()))?,
    );
    let calc = state
        .db
        .get_calculation(calc_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let path = calc.result_path.ok_or(AppError::NotFound)?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| AppError::NotFound)?;

    Ok((
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
        bytes,
    ))
}
