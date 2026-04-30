use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::model::{CalcStatus, ErrorKind};
use crate::types::{CalcId, RunId};

/// Events emitted by the server onto the Event Bus and stored in the `events` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServerEvent {
    RunSubmitted {
        run_id: RunId,
        jira_issue_id: String,
        at: DateTime<Utc>,
    },
    CalcStatusChanged {
        run_id: RunId,
        calculation_id: CalcId,
        from: CalcStatus,
        to: CalcStatus,
        attempt: u32,
        at: DateTime<Utc>,
    },
    CalcProgress {
        run_id: RunId,
        calculation_id: CalcId,
        fraction: f32,
        note: Option<String>,
        at: DateTime<Utc>,
    },
    CalcCompleted {
        run_id: RunId,
        calculation_id: CalcId,
        result_path: PathBuf,
        at: DateTime<Utc>,
    },
    CalcFailed {
        run_id: RunId,
        calculation_id: CalcId,
        error_kind: ErrorKind,
        message: String,
        retriable: bool,
        at: DateTime<Utc>,
    },
}

impl ServerEvent {
    pub fn run_id(&self) -> Option<&RunId> {
        match self {
            Self::RunSubmitted { run_id, .. } => Some(run_id),
            Self::CalcStatusChanged { run_id, .. } => Some(run_id),
            Self::CalcProgress { run_id, .. } => Some(run_id),
            Self::CalcCompleted { run_id, .. } => Some(run_id),
            Self::CalcFailed { run_id, .. } => Some(run_id),
        }
    }

    pub fn calc_id(&self) -> Option<&CalcId> {
        match self {
            Self::RunSubmitted { .. } => None,
            Self::CalcStatusChanged { calculation_id, .. } => Some(calculation_id),
            Self::CalcProgress { calculation_id, .. } => Some(calculation_id),
            Self::CalcCompleted { calculation_id, .. } => Some(calculation_id),
            Self::CalcFailed { calculation_id, .. } => Some(calculation_id),
        }
    }

    pub fn event_kind_str(&self) -> &'static str {
        match self {
            Self::RunSubmitted { .. } => "run.submitted",
            Self::CalcStatusChanged { .. } => "calculation.status_changed",
            Self::CalcProgress { .. } => "calculation.progress",
            Self::CalcCompleted { .. } => "calculation.completed",
            Self::CalcFailed { .. } => "calculation.failed",
        }
    }
}

/// An envelope that carries a DB sequence number alongside the event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequencedEvent {
    pub seq: i64,
    pub event: ServerEvent,
}
