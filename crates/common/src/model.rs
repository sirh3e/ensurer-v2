use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::{CalcId, RunId};

// ── Status enums ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalcStatus {
    Pending,
    Running,
    Retrying,
    Succeeded,
    Failed,
    Cancelled,
}

impl CalcStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

impl std::fmt::Display for CalcStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Retrying => "retrying",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        };
        write!(f, "{s}")
    }
}

impl std::str::FromStr for CalcStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "retrying" => Ok(Self::Retrying),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(format!("unknown calc status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    PartiallySucceeded,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::PartiallySucceeded => "partially_succeeded",
        };
        write!(f, "{s}")
    }
}

impl std::str::FromStr for RunStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "partially_succeeded" => Ok(Self::PartiallySucceeded),
            other => Err(format!("unknown run status: {other}")),
        }
    }
}

/// Derive the Run status purely from its Calculations' statuses.
pub fn derive_run_status(calc_statuses: &[CalcStatus]) -> RunStatus {
    if calc_statuses.is_empty() {
        return RunStatus::Pending;
    }
    let any_active = calc_statuses.iter().any(|s| {
        matches!(
            s,
            CalcStatus::Running | CalcStatus::Retrying | CalcStatus::Pending
        )
    });
    if any_active {
        return RunStatus::Running;
    }
    let succeeded = calc_statuses
        .iter()
        .filter(|s| **s == CalcStatus::Succeeded)
        .count();
    let failed = calc_statuses
        .iter()
        .filter(|s| **s == CalcStatus::Failed)
        .count();
    let cancelled = calc_statuses
        .iter()
        .filter(|s| **s == CalcStatus::Cancelled)
        .count();
    let total = calc_statuses.len();

    if succeeded == total {
        RunStatus::Succeeded
    } else if cancelled == total {
        RunStatus::Cancelled
    } else if failed == total {
        RunStatus::Failed
    } else {
        RunStatus::PartiallySucceeded
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Transient,
    TransientExhausted,
    Permanent,
    CrashExhausted,
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Transient => "transient",
            Self::TransientExhausted => "transient_exhausted",
            Self::Permanent => "permanent",
            Self::CrashExhausted => "crash_exhausted",
        };
        write!(f, "{s}")
    }
}

impl std::str::FromStr for ErrorKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "transient" => Ok(Self::Transient),
            "transient_exhausted" => Ok(Self::TransientExhausted),
            "permanent" => Ok(Self::Permanent),
            "crash_exhausted" => Ok(Self::CrashExhausted),
            other => Err(format!("unknown error kind: {other}")),
        }
    }
}

// ── Domain structs ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: RunId,
    pub jira_issue_id: String,
    pub submitted_by: String,
    pub status: RunStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub calculations: Vec<Calculation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Calculation {
    pub id: CalcId,
    pub run_id: RunId,
    pub kind: String,
    pub input_json: serde_json::Value,
    pub idempotency_key: String,
    pub status: CalcStatus,
    pub attempt: u32,
    pub max_attempts: u32,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub error_kind: Option<ErrorKind>,
    pub error_message: Option<String>,
    pub result_path: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewCalc {
    pub kind: String,
    pub input: serde_json::Value,
}

// ── API request / response types ──────────────────────────────────────────────

// ── Request validation ────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("{0}")]
    Field(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubmitRunRequest {
    pub jira_issue_id: String,
    pub calculations: Vec<NewCalc>,
}

impl SubmitRunRequest {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.jira_issue_id.trim().is_empty() {
            return Err(ValidationError::Field(
                "jira_issue_id must not be empty".into(),
            ));
        }
        if self.calculations.is_empty() {
            return Err(ValidationError::Field(
                "at least one calculation is required".into(),
            ));
        }
        for (i, calc) in self.calculations.iter().enumerate() {
            if calc.kind.trim().is_empty() {
                return Err(ValidationError::Field(format!(
                    "calculations[{i}].kind must not be empty"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubmitRunResponse {
    pub run_id: RunId,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListRunsQuery {
    pub status: Option<String>,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListRunsResponse {
    pub runs: Vec<Run>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProblemDetail {
    #[serde(rename = "type")]
    pub type_uri: String,
    pub title: String,
    pub status: u16,
    pub detail: Option<String>,
}
