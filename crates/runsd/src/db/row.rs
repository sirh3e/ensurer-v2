/// Raw SQLite row types and their fallible conversions to domain types.
use chrono::{DateTime, Utc};
use thiserror::Error;

use common::{
    model::{CalcStatus, Calculation, ErrorKind, Run, RunStatus},
    types::{CalcId, RunId},
};
use std::str::FromStr;

// ── Conversion error ──────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum RowConversionError {
    #[error("invalid UUID in column '{column}': {source}")]
    InvalidUuid {
        column: &'static str,
        source: uuid::Error,
    },
    #[error("unknown value '{value}' in column '{column}'")]
    UnknownVariant { column: &'static str, value: String },
    #[error("invalid JSON in column '{column}': {source}")]
    InvalidJson {
        column: &'static str,
        source: serde_json::Error,
    },
    #[error("timestamp out of range in column '{column}': {millis}ms")]
    InvalidTimestamp { column: &'static str, millis: i64 },
}

fn millis_to_dt(ms: i64, column: &'static str) -> Result<DateTime<Utc>, RowConversionError> {
    DateTime::from_timestamp_millis(ms)
        .ok_or(RowConversionError::InvalidTimestamp { column, millis: ms })
}

// ── Run row ───────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow, Debug)]
pub struct RunRow {
    pub id: String,
    pub jira_issue_id: String,
    pub submitted_by: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl RunRow {
    /// Parse only the primary key — needed before fetching child calculations.
    pub fn parse_id(&self) -> Result<RunId, RowConversionError> {
        self.id
            .parse()
            .map(RunId)
            .map_err(|e| RowConversionError::InvalidUuid {
                column: "runs.id",
                source: e,
            })
    }

    /// Convert into a domain `Run`, supplying pre-fetched calculations.
    pub fn try_into_run(self, calculations: Vec<Calculation>) -> Result<Run, RowConversionError> {
        let id = self
            .id
            .parse()
            .map(RunId)
            .map_err(|e| RowConversionError::InvalidUuid {
                column: "runs.id",
                source: e,
            })?;
        let status =
            RunStatus::from_str(&self.status).map_err(|_| RowConversionError::UnknownVariant {
                column: "runs.status",
                value: self.status.clone(),
            })?;
        Ok(Run {
            id,
            jira_issue_id: self.jira_issue_id,
            submitted_by: self.submitted_by,
            status,
            created_at: millis_to_dt(self.created_at, "runs.created_at")?,
            updated_at: millis_to_dt(self.updated_at, "runs.updated_at")?,
            calculations,
        })
    }
}

// ── Calculation row ───────────────────────────────────────────────────────────

#[derive(sqlx::FromRow, Debug)]
pub struct CalcRow {
    pub id: String,
    pub run_id: String,
    pub kind: String,
    pub input_json: String,
    pub idempotency_key: String,
    pub status: String,
    pub attempt: i64,
    pub max_attempts: i64,
    pub next_attempt_at: Option<i64>,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<i64>,
    pub error_kind: Option<String>,
    pub error_message: Option<String>,
    pub result_path: Option<String>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub updated_at: i64,
}

impl CalcRow {
    /// Convert into a domain `Calculation`, returning an error on corrupt data.
    pub fn try_into_calc(self) -> Result<Calculation, RowConversionError> {
        let id = self
            .id
            .parse()
            .map(CalcId)
            .map_err(|e| RowConversionError::InvalidUuid {
                column: "calculations.id",
                source: e,
            })?;
        let run_id =
            self.run_id
                .parse()
                .map(RunId)
                .map_err(|e| RowConversionError::InvalidUuid {
                    column: "calculations.run_id",
                    source: e,
                })?;
        let status =
            CalcStatus::from_str(&self.status).map_err(|_| RowConversionError::UnknownVariant {
                column: "calculations.status",
                value: self.status.clone(),
            })?;
        let input_json: serde_json::Value =
            serde_json::from_str(&self.input_json).map_err(|e| {
                RowConversionError::InvalidJson {
                    column: "calculations.input_json",
                    source: e,
                }
            })?;
        let error_kind = self
            .error_kind
            .map(|s| {
                ErrorKind::from_str(&s).map_err(|_| RowConversionError::UnknownVariant {
                    column: "calculations.error_kind",
                    value: s,
                })
            })
            .transpose()?;

        Ok(Calculation {
            id,
            run_id,
            kind: self.kind,
            input_json,
            idempotency_key: self.idempotency_key,
            status,
            attempt: self.attempt as u32,
            max_attempts: self.max_attempts as u32,
            next_attempt_at: self
                .next_attempt_at
                .map(|ms| millis_to_dt(ms, "calculations.next_attempt_at"))
                .transpose()?,
            lease_owner: self.lease_owner,
            lease_expires_at: self
                .lease_expires_at
                .map(|ms| millis_to_dt(ms, "calculations.lease_expires_at"))
                .transpose()?,
            error_kind,
            error_message: self.error_message,
            result_path: self.result_path,
            created_at: millis_to_dt(self.created_at, "calculations.created_at")?,
            started_at: self
                .started_at
                .map(|ms| millis_to_dt(ms, "calculations.started_at"))
                .transpose()?,
            completed_at: self
                .completed_at
                .map(|ms| millis_to_dt(ms, "calculations.completed_at"))
                .transpose()?,
            updated_at: millis_to_dt(self.updated_at, "calculations.updated_at")?,
        })
    }
}

// ── Event row ─────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow, Debug)]
pub struct EventRow {
    pub seq: i64,
    pub run_id: Option<String>,
    pub calculation_id: Option<String>,
    pub kind: String,
    pub payload_json: String,
    pub created_at: i64,
}

// ── Timestamp helpers ─────────────────────────────────────────────────────────

pub fn dt_to_millis(dt: DateTime<Utc>) -> i64 {
    dt.timestamp_millis()
}

pub fn now_millis() -> i64 {
    Utc::now().timestamp_millis()
}
