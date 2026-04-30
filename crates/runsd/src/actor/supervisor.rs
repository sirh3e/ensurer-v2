use std::{collections::HashMap, sync::Arc};

use chrono::Utc;
use tokio::sync::{mpsc, oneshot};
use tracing::info;

use common::{
    idempotency::compute_idempotency_key,
    model::{CalcStatus, Calculation, NewCalc},
    types::{CalcId, RunId},
};

use crate::{
    actor::{
        calc::{CalcActor, CalcCmd, ReqwestCalcClient},
        db::DbHandle,
        event_bus::EventBus,
        run::{RunActor, RunCmd},
        worker_pool::WorkerPool,
    },
    config::Config,
    error::AppError,
};

pub const SUBMIT_MAILBOX_CAPACITY: usize = 64;

// ── Commands ──────────────────────────────────────────────────────────────────

pub enum SupervisorCmd {
    SubmitRun {
        jira_issue_id: String,
        submitted_by: String,
        calcs: Vec<NewCalc>,
        reply: oneshot::Sender<Result<RunId, AppError>>,
    },
    CancelRun {
        run_id: RunId,
        reply: oneshot::Sender<Result<(), AppError>>,
    },
    CancelCalc {
        run_id: RunId,
        calc_id: CalcId,
        reply: oneshot::Sender<Result<(), AppError>>,
    },
    RescheduleCalc {
        run_id: RunId,
        calc_id: CalcId,
    },
    RetryCalc {
        run_id: RunId,
        calc_id: CalcId,
        reply: oneshot::Sender<Result<(), AppError>>,
    },
    Shutdown,
}

// ── Handle ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct SupervisorHandle {
    tx: mpsc::Sender<SupervisorCmd>,
}

impl SupervisorHandle {
    pub fn new(tx: mpsc::Sender<SupervisorCmd>) -> Self {
        Self { tx }
    }

    pub async fn submit_run(
        &self,
        jira_issue_id: String,
        submitted_by: String,
        calcs: Vec<NewCalc>,
    ) -> Result<RunId, AppError> {
        let (reply, rx) = oneshot::channel();
        if self
            .tx
            .send_timeout(
                SupervisorCmd::SubmitRun {
                    jira_issue_id,
                    submitted_by,
                    calcs,
                    reply,
                },
                std::time::Duration::from_millis(100),
            )
            .await
            .is_err()
        {
            return Err(AppError::ServiceUnavailable(
                "supervisor mailbox full".into(),
            ));
        }
        rx.await
            .unwrap_or(Err(AppError::Internal("supervisor gone".into())))
    }

    pub async fn cancel_run(&self, run_id: RunId) -> Result<(), AppError> {
        let (reply, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(SupervisorCmd::CancelRun { run_id, reply })
            .await;
        rx.await
            .unwrap_or(Err(AppError::Internal("supervisor gone".into())))
    }

    pub async fn cancel_calc(&self, run_id: RunId, calc_id: CalcId) -> Result<(), AppError> {
        let (reply, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(SupervisorCmd::CancelCalc {
                run_id,
                calc_id,
                reply,
            })
            .await;
        rx.await
            .unwrap_or(Err(AppError::Internal("supervisor gone".into())))
    }

    pub fn reschedule_calc(&self, run_id: RunId, calc_id: CalcId) {
        let _ = self
            .tx
            .try_send(SupervisorCmd::RescheduleCalc { run_id, calc_id });
    }

    pub async fn retry_calc(&self, run_id: RunId, calc_id: CalcId) -> Result<(), AppError> {
        let (reply, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(SupervisorCmd::RetryCalc {
                run_id,
                calc_id,
                reply,
            })
            .await;
        rx.await
            .unwrap_or(Err(AppError::Internal("supervisor gone".into())))
    }

    pub async fn shutdown(&self) {
        let _ = self.tx.send(SupervisorCmd::Shutdown).await;
    }
}

// ── Supervisor ────────────────────────────────────────────────────────────────

pub struct Supervisor {
    db: DbHandle,
    bus: EventBus,
    pool: WorkerPool,
    http: reqwest::Client,
    config: Arc<Config>,
    run_actors: HashMap<RunId, mpsc::Sender<RunCmd>>,
    // calc_id → (run_id, cmd_tx) for running calcs not inside a RunActor
    calc_actors: HashMap<CalcId, (RunId, mpsc::Sender<CalcCmd>)>,
}

impl Supervisor {
    pub fn new(
        db: DbHandle,
        bus: EventBus,
        pool: WorkerPool,
        http: reqwest::Client,
        config: Arc<Config>,
    ) -> Self {
        Self {
            db,
            bus,
            pool,
            http,
            config,
            run_actors: HashMap::new(),
            calc_actors: HashMap::new(),
        }
    }

    pub fn spawn(self) -> SupervisorHandle {
        let (tx, rx) = mpsc::channel(SUBMIT_MAILBOX_CAPACITY);
        let handle = SupervisorHandle::new(tx);
        tokio::spawn(async move { self.run(rx).await });
        handle
    }

    async fn run(mut self, mut rx: mpsc::Receiver<SupervisorCmd>) {
        while let Some(cmd) = rx.recv().await {
            match cmd {
                SupervisorCmd::SubmitRun {
                    jira_issue_id,
                    submitted_by,
                    calcs,
                    reply,
                } => {
                    let result = self.submit_run(jira_issue_id, submitted_by, calcs).await;
                    let _ = reply.send(result);
                }
                SupervisorCmd::CancelRun { run_id, reply } => {
                    let result = self.cancel_run(&run_id).await;
                    let _ = reply.send(result);
                }
                SupervisorCmd::CancelCalc {
                    run_id,
                    calc_id,
                    reply,
                } => {
                    let result = self.cancel_calc(&run_id, &calc_id).await;
                    let _ = reply.send(result);
                }
                SupervisorCmd::RescheduleCalc { run_id, calc_id } => {
                    self.reschedule_calc(run_id, calc_id).await;
                }
                SupervisorCmd::RetryCalc {
                    run_id,
                    calc_id,
                    reply,
                } => {
                    let result = self.retry_calc(run_id, calc_id).await;
                    let _ = reply.send(result);
                }
                SupervisorCmd::Shutdown => break,
            }
        }
        info!("supervisor shut down");
    }

    async fn submit_run(
        &mut self,
        jira_issue_id: String,
        submitted_by: String,
        new_calcs: Vec<NewCalc>,
    ) -> Result<RunId, AppError> {
        let run_id = RunId::new();
        self.db
            .insert_run(run_id.clone(), jira_issue_id.clone(), submitted_by.clone())
            .await?;

        let now = Utc::now();
        let mut calc_specs = Vec::new();
        for nc in new_calcs {
            let calc_id = CalcId::new();
            let idem_key = compute_idempotency_key(&nc.kind, &nc.input);
            let calc = Calculation {
                id: calc_id.clone(),
                run_id: run_id.clone(),
                kind: nc.kind.clone(),
                input_json: nc.input.clone(),
                idempotency_key: idem_key.clone(),
                status: CalcStatus::Pending,
                attempt: 0,
                max_attempts: self.config.retry.max_attempts,
                next_attempt_at: None,
                lease_owner: None,
                lease_expires_at: None,
                error_kind: None,
                error_message: None,
                result_path: None,
                created_at: now,
                started_at: None,
                completed_at: None,
                updated_at: now,
            };
            self.db.insert_calculation(calc).await?;
            calc_specs.push((
                calc_id,
                nc.kind,
                nc.input,
                idem_key,
                0u32,
                self.config.retry.max_attempts,
            ));
        }

        self.spawn_run_actor(run_id.clone(), calc_specs);
        Ok(run_id)
    }

    fn spawn_run_actor(
        &mut self,
        run_id: RunId,
        calcs: Vec<(CalcId, String, serde_json::Value, String, u32, u32)>,
    ) {
        let actor = RunActor {
            run_id: run_id.clone(),
            db: self.db.clone(),
            bus: self.bus.clone(),
            pool: self.pool.clone(),
            http: self.http.clone(),
            config: Arc::clone(&self.config),
            calcs,
        };
        let tx = actor.spawn();
        self.run_actors.insert(run_id, tx);
    }

    async fn cancel_run(&mut self, run_id: &RunId) -> Result<(), AppError> {
        if let Some(tx) = self.run_actors.remove(run_id) {
            let _ = tx.send(RunCmd::Cancel).await;
        }
        Ok(())
    }

    async fn cancel_calc(&mut self, _run_id: &RunId, calc_id: &CalcId) -> Result<(), AppError> {
        if let Some((_, tx)) = self.calc_actors.remove(calc_id) {
            let _ = tx.send(CalcCmd::Cancel).await;
        }
        Ok(())
    }

    async fn reschedule_calc(&mut self, run_id: RunId, calc_id: CalcId) {
        if let Ok(Some(calc)) = self.db.get_calculation(calc_id.clone()).await {
            let (notif_tx, _) = mpsc::channel(1);
            let api: Arc<dyn crate::actor::calc::CalcApiClient> = Arc::new(ReqwestCalcClient {
                http: self.http.clone(),
                cfg: self.config.external_api.clone(),
                data_dir: self.config.server.data_dir.clone(),
            });
            let actor = CalcActor {
                id: calc.id.clone(),
                run_id: run_id.clone(),
                kind: calc.kind.clone(),
                input_json: calc.input_json.clone(),
                idempotency_key: calc.idempotency_key.clone(),
                attempt: calc.attempt,
                max_attempts: calc.max_attempts,
                db: self.db.clone(),
                bus: self.bus.clone(),
                run_tx: notif_tx,
                pool: self.pool.clone(),
                api,
                retry_cfg: self.config.retry.clone(),
                lease_cfg: self.config.lease.clone(),
                data_dir: self.config.server.data_dir.clone(),
                worker_id: format!("watchdog-{}", calc.id),
            };
            let tx = actor.spawn();
            let _ = tx.send(CalcCmd::Start).await;
            self.calc_actors.insert(calc_id, (run_id, tx));
        }
    }

    async fn retry_calc(&mut self, run_id: RunId, calc_id: CalcId) -> Result<(), AppError> {
        // Reset the calculation to pending attempt 0.
        self.db.calc_reset_pending(calc_id.clone()).await?;
        self.reschedule_calc(run_id, calc_id).await;
        Ok(())
    }

    /// Called at startup to re-spawn actors for runs that are still active.
    pub async fn restore_active_runs(&mut self) -> Result<(), AppError> {
        let run_ids = self.db.list_active_run_ids().await?;
        for run_id in run_ids {
            if let Ok(calcs) = self.db.list_calculations_for_run(run_id.clone()).await {
                let specs: Vec<_> = calcs
                    .into_iter()
                    .filter(|c| !c.status.is_terminal())
                    .map(|c| {
                        (
                            c.id,
                            c.kind,
                            c.input_json,
                            c.idempotency_key,
                            c.attempt,
                            c.max_attempts,
                        )
                    })
                    .collect();
                if !specs.is_empty() {
                    self.spawn_run_actor(run_id, specs);
                }
            }
        }
        Ok(())
    }
}
