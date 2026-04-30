use tokio::sync::{mpsc, oneshot};
use tracing::error;

use common::{
    model::{CalcStatus, Calculation, Run, RunStatus},
    types::{CalcId, RunId},
};

use crate::{
    db::queries::{self, Db},
    error::AppError,
};

pub const MAILBOX_CAPACITY: usize = 256;

// ── Commands ──────────────────────────────────────────────────────────────────

pub enum DbCmd {
    InsertRun {
        id: RunId,
        jira_issue_id: String,
        submitted_by: String,
        reply: oneshot::Sender<Result<(), AppError>>,
    },
    UpdateRunStatus {
        id: RunId,
        status: RunStatus,
        reply: oneshot::Sender<Result<(), AppError>>,
    },
    GetRun {
        id: RunId,
        reply: oneshot::Sender<Result<Option<Run>, AppError>>,
    },
    ListRuns {
        status_filter: Option<String>,
        limit: u32,
        cursor_created_at: Option<i64>,
        cursor_id: Option<String>,
        reply: oneshot::Sender<Result<Vec<Run>, AppError>>,
    },
    InsertCalculation {
        calc: Calculation,
        reply: oneshot::Sender<Result<(), AppError>>,
    },
    GetCalculation {
        id: CalcId,
        reply: oneshot::Sender<Result<Option<Calculation>, AppError>>,
    },
    ListCalculationsForRun {
        run_id: RunId,
        reply: oneshot::Sender<Result<Vec<Calculation>, AppError>>,
    },
    CalcStarted {
        id: CalcId,
        lease_owner: String,
        lease_expires_at: i64,
        reply: oneshot::Sender<Result<(), AppError>>,
    },
    CalcHeartbeat {
        id: CalcId,
        lease_expires_at: i64,
        reply: oneshot::Sender<Result<(), AppError>>,
    },
    CalcSucceeded {
        id: CalcId,
        result_path: String,
        reply: oneshot::Sender<Result<(), AppError>>,
    },
    CalcFailed {
        id: CalcId,
        error_kind: String,
        error_message: String,
        reply: oneshot::Sender<Result<(), AppError>>,
    },
    CalcRetrying {
        id: CalcId,
        attempt: u32,
        next_attempt_at: i64,
        reply: oneshot::Sender<Result<(), AppError>>,
    },
    CalcCancelled {
        id: CalcId,
        reply: oneshot::Sender<Result<(), AppError>>,
    },
    CalcResetPending {
        id: CalcId,
        reply: oneshot::Sender<Result<(), AppError>>,
    },
    InsertEvent {
        run_id: Option<String>,
        calculation_id: Option<String>,
        kind: String,
        payload_json: String,
        reply: oneshot::Sender<Result<i64, AppError>>,
    },
    ListExpiredLeases {
        now: i64,
        reply: oneshot::Sender<Result<Vec<Calculation>, AppError>>,
    },
    ListReadyRetries {
        now: i64,
        reply: oneshot::Sender<Result<Vec<Calculation>, AppError>>,
    },
    ListActiveRunIds {
        reply: oneshot::Sender<Result<Vec<RunId>, AppError>>,
    },
    GetCalcStatusesForRun {
        run_id: RunId,
        reply: oneshot::Sender<Result<Vec<CalcStatus>, AppError>>,
    },
    PruneEvents {
        older_than_ms: i64,
        reply: oneshot::Sender<Result<u64, AppError>>,
    },
}

// ── Handle ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct DbHandle {
    tx: mpsc::Sender<DbCmd>,
}

impl DbHandle {
    fn new(tx: mpsc::Sender<DbCmd>) -> Self {
        Self { tx }
    }

    async fn send(&self, cmd: DbCmd) {
        if self.tx.send(cmd).await.is_err() {
            error!("DbActor mailbox closed");
        }
    }

    pub async fn insert_run(
        &self,
        id: RunId,
        jira_issue_id: String,
        submitted_by: String,
    ) -> Result<(), AppError> {
        let (reply, rx) = oneshot::channel();
        self.send(DbCmd::InsertRun { id, jira_issue_id, submitted_by, reply }).await;
        rx.await.unwrap_or(Err(AppError::Internal("db actor gone".into())))
    }

    pub async fn update_run_status(&self, id: RunId, status: RunStatus) -> Result<(), AppError> {
        let (reply, rx) = oneshot::channel();
        self.send(DbCmd::UpdateRunStatus { id, status, reply }).await;
        rx.await.unwrap_or(Err(AppError::Internal("db actor gone".into())))
    }

    pub async fn get_run(&self, id: RunId) -> Result<Option<Run>, AppError> {
        let (reply, rx) = oneshot::channel();
        self.send(DbCmd::GetRun { id, reply }).await;
        rx.await.unwrap_or(Err(AppError::Internal("db actor gone".into())))
    }

    pub async fn list_runs(
        &self,
        status_filter: Option<String>,
        limit: u32,
        cursor_created_at: Option<i64>,
        cursor_id: Option<String>,
    ) -> Result<Vec<Run>, AppError> {
        let (reply, rx) = oneshot::channel();
        self.send(DbCmd::ListRuns { status_filter, limit, cursor_created_at, cursor_id, reply })
            .await;
        rx.await.unwrap_or(Err(AppError::Internal("db actor gone".into())))
    }

    pub async fn insert_calculation(&self, calc: Calculation) -> Result<(), AppError> {
        let (reply, rx) = oneshot::channel();
        self.send(DbCmd::InsertCalculation { calc, reply }).await;
        rx.await.unwrap_or(Err(AppError::Internal("db actor gone".into())))
    }

    pub async fn get_calculation(&self, id: CalcId) -> Result<Option<Calculation>, AppError> {
        let (reply, rx) = oneshot::channel();
        self.send(DbCmd::GetCalculation { id, reply }).await;
        rx.await.unwrap_or(Err(AppError::Internal("db actor gone".into())))
    }

    pub async fn list_calculations_for_run(
        &self,
        run_id: RunId,
    ) -> Result<Vec<Calculation>, AppError> {
        let (reply, rx) = oneshot::channel();
        self.send(DbCmd::ListCalculationsForRun { run_id, reply }).await;
        rx.await.unwrap_or(Err(AppError::Internal("db actor gone".into())))
    }

    pub async fn calc_started(
        &self,
        id: CalcId,
        lease_owner: String,
        lease_expires_at: i64,
    ) -> Result<(), AppError> {
        let (reply, rx) = oneshot::channel();
        self.send(DbCmd::CalcStarted { id, lease_owner, lease_expires_at, reply }).await;
        rx.await.unwrap_or(Err(AppError::Internal("db actor gone".into())))
    }

    pub async fn calc_heartbeat(&self, id: CalcId, lease_expires_at: i64) -> Result<(), AppError> {
        let (reply, rx) = oneshot::channel();
        self.send(DbCmd::CalcHeartbeat { id, lease_expires_at, reply }).await;
        rx.await.unwrap_or(Err(AppError::Internal("db actor gone".into())))
    }

    pub async fn calc_succeeded(&self, id: CalcId, result_path: String) -> Result<(), AppError> {
        let (reply, rx) = oneshot::channel();
        self.send(DbCmd::CalcSucceeded { id, result_path, reply }).await;
        rx.await.unwrap_or(Err(AppError::Internal("db actor gone".into())))
    }

    pub async fn calc_failed(
        &self,
        id: CalcId,
        error_kind: String,
        error_message: String,
    ) -> Result<(), AppError> {
        let (reply, rx) = oneshot::channel();
        self.send(DbCmd::CalcFailed { id, error_kind, error_message, reply }).await;
        rx.await.unwrap_or(Err(AppError::Internal("db actor gone".into())))
    }

    pub async fn calc_retrying(
        &self,
        id: CalcId,
        attempt: u32,
        next_attempt_at: i64,
    ) -> Result<(), AppError> {
        let (reply, rx) = oneshot::channel();
        self.send(DbCmd::CalcRetrying { id, attempt, next_attempt_at, reply }).await;
        rx.await.unwrap_or(Err(AppError::Internal("db actor gone".into())))
    }

    pub async fn calc_cancelled(&self, id: CalcId) -> Result<(), AppError> {
        let (reply, rx) = oneshot::channel();
        self.send(DbCmd::CalcCancelled { id, reply }).await;
        rx.await.unwrap_or(Err(AppError::Internal("db actor gone".into())))
    }

    pub async fn calc_reset_pending(&self, id: CalcId) -> Result<(), AppError> {
        let (reply, rx) = oneshot::channel();
        self.send(DbCmd::CalcResetPending { id, reply }).await;
        rx.await.unwrap_or(Err(AppError::Internal("db actor gone".into())))
    }

    pub async fn insert_event(
        &self,
        run_id: Option<String>,
        calculation_id: Option<String>,
        kind: String,
        payload_json: String,
    ) -> Result<i64, AppError> {
        let (reply, rx) = oneshot::channel();
        self.send(DbCmd::InsertEvent { run_id, calculation_id, kind, payload_json, reply }).await;
        rx.await.unwrap_or(Err(AppError::Internal("db actor gone".into())))
    }

    pub async fn list_expired_leases(&self, now: i64) -> Result<Vec<Calculation>, AppError> {
        let (reply, rx) = oneshot::channel();
        self.send(DbCmd::ListExpiredLeases { now, reply }).await;
        rx.await.unwrap_or(Err(AppError::Internal("db actor gone".into())))
    }

    pub async fn list_ready_retries(&self, now: i64) -> Result<Vec<Calculation>, AppError> {
        let (reply, rx) = oneshot::channel();
        self.send(DbCmd::ListReadyRetries { now, reply }).await;
        rx.await.unwrap_or(Err(AppError::Internal("db actor gone".into())))
    }

    pub async fn list_active_run_ids(&self) -> Result<Vec<RunId>, AppError> {
        let (reply, rx) = oneshot::channel();
        self.send(DbCmd::ListActiveRunIds { reply }).await;
        rx.await.unwrap_or(Err(AppError::Internal("db actor gone".into())))
    }

    pub async fn get_calc_statuses_for_run(
        &self,
        run_id: RunId,
    ) -> Result<Vec<CalcStatus>, AppError> {
        let (reply, rx) = oneshot::channel();
        self.send(DbCmd::GetCalcStatusesForRun { run_id, reply }).await;
        rx.await.unwrap_or(Err(AppError::Internal("db actor gone".into())))
    }

    pub async fn prune_events(&self, older_than_ms: i64) -> Result<u64, AppError> {
        let (reply, rx) = oneshot::channel();
        self.send(DbCmd::PruneEvents { older_than_ms, reply }).await;
        rx.await.unwrap_or(Err(AppError::Internal("db actor gone".into())))
    }
}

// ── Actor loop ────────────────────────────────────────────────────────────────

/// Spawn the DB Actor. Returns a handle for sending commands.
/// The actor owns the write pool; callers supply a read pool for read-only queries.
pub fn spawn(write_pool: Db) -> DbHandle {
    let (tx, mut rx) = mpsc::channel::<DbCmd>(MAILBOX_CAPACITY);
    tokio::spawn(async move {
        while let Some(cmd) = rx.recv().await {
            process(&write_pool, cmd).await;
        }
    });
    DbHandle::new(tx)
}

async fn process(db: &Db, cmd: DbCmd) {
    match cmd {
        DbCmd::InsertRun { id, jira_issue_id, submitted_by, reply } => {
            let _ = reply.send(queries::insert_run(db, &id, &jira_issue_id, &submitted_by).await);
        }
        DbCmd::UpdateRunStatus { id, status, reply } => {
            let _ = reply.send(queries::update_run_status(db, &id, status).await);
        }
        DbCmd::GetRun { id, reply } => {
            let _ = reply.send(queries::get_run(db, &id).await);
        }
        DbCmd::ListRuns { status_filter, limit, cursor_created_at, cursor_id, reply } => {
            let _ = reply.send(
                queries::list_runs(
                    db,
                    status_filter.as_deref(),
                    limit,
                    cursor_created_at,
                    cursor_id.as_deref(),
                )
                .await,
            );
        }
        DbCmd::InsertCalculation { calc, reply } => {
            let _ = reply.send(queries::insert_calculation(db, &calc).await);
        }
        DbCmd::GetCalculation { id, reply } => {
            let _ = reply.send(queries::get_calculation(db, &id).await);
        }
        DbCmd::ListCalculationsForRun { run_id, reply } => {
            let _ = reply.send(queries::list_calculations_for_run(db, &run_id).await);
        }
        DbCmd::CalcStarted { id, lease_owner, lease_expires_at, reply } => {
            let _ = reply.send(
                queries::update_calc_started(db, &id, &lease_owner, lease_expires_at).await,
            );
        }
        DbCmd::CalcHeartbeat { id, lease_expires_at, reply } => {
            let _ = reply.send(queries::update_calc_heartbeat(db, &id, lease_expires_at).await);
        }
        DbCmd::CalcSucceeded { id, result_path, reply } => {
            let _ = reply.send(queries::update_calc_succeeded(db, &id, &result_path).await);
        }
        DbCmd::CalcFailed { id, error_kind, error_message, reply } => {
            let _ = reply.send(
                queries::update_calc_failed(db, &id, &error_kind, &error_message).await,
            );
        }
        DbCmd::CalcRetrying { id, attempt, next_attempt_at, reply } => {
            let _ = reply.send(
                queries::update_calc_retrying(db, &id, attempt, next_attempt_at).await,
            );
        }
        DbCmd::CalcCancelled { id, reply } => {
            let _ = reply.send(queries::update_calc_cancelled(db, &id).await);
        }
        DbCmd::CalcResetPending { id, reply } => {
            let _ = reply.send(queries::update_calc_pending(db, &id).await);
        }
        DbCmd::InsertEvent { run_id, calculation_id, kind, payload_json, reply } => {
            let _ = reply.send(
                queries::insert_event(
                    db,
                    run_id.as_deref(),
                    calculation_id.as_deref(),
                    &kind,
                    &payload_json,
                )
                .await,
            );
        }
        DbCmd::ListExpiredLeases { now, reply } => {
            let _ = reply.send(queries::list_expired_leases(db, now).await);
        }
        DbCmd::ListReadyRetries { now, reply } => {
            let _ = reply.send(queries::list_ready_retries(db, now).await);
        }
        DbCmd::ListActiveRunIds { reply } => {
            let _ = reply.send(queries::list_active_run_ids(db).await);
        }
        DbCmd::GetCalcStatusesForRun { run_id, reply } => {
            let _ = reply.send(queries::get_calc_statuses_for_run(db, &run_id).await);
        }
        DbCmd::PruneEvents { older_than_ms, reply } => {
            let _ = reply.send(queries::prune_old_events(db, older_than_ms).await);
        }
    }
}
