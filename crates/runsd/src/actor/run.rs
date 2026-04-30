use std::{collections::HashMap, sync::Arc};

use tokio::sync::mpsc;
use tracing::{error, info};

use common::{
    model::derive_run_status,
    types::{CalcId, RunId},
};

use crate::{
    actor::{
        calc::{CalcActor, CalcCmd},
        db::DbHandle,
        event_bus::EventBus,
        worker_pool::WorkerPool,
        calc::ReqwestCalcClient,
    },
    config::Config,
};

// ── Notification from child calcs ─────────────────────────────────────────────

#[derive(Debug)]
pub enum RunNotification {
    CalcFinished(CalcId),
    Cancel,
}

// ── Commands ──────────────────────────────────────────────────────────────────

pub enum RunCmd {
    Cancel,
}

// ── Actor ─────────────────────────────────────────────────────────────────────

pub struct RunActor {
    pub run_id: RunId,
    pub db: DbHandle,
    pub bus: EventBus,
    pub pool: WorkerPool,
    pub http: reqwest::Client,
    pub config: Arc<Config>,
    pub calcs: Vec<(CalcId, String, serde_json::Value, String, u32, u32)>, // id,kind,input,idem_key,attempt,max
}

impl RunActor {
    pub fn spawn(self) -> mpsc::Sender<RunCmd> {
        let (tx, rx) = mpsc::channel(8);
        tokio::spawn(async move { self.run(rx).await });
        tx
    }

    async fn run(self, mut cmd_rx: mpsc::Receiver<RunCmd>) {
        let (notif_tx, mut notif_rx) = mpsc::channel::<RunNotification>(64);
        let mut calc_txs: HashMap<CalcId, mpsc::Sender<CalcCmd>> = HashMap::new();
        let worker_id = format!("worker-{}", self.run_id);

        // Spawn a CalcActor for each calculation.
        for (calc_id, kind, input_json, idem_key, attempt, max_attempts) in self.calcs.iter() {
            let api: Arc<dyn crate::actor::calc::CalcApiClient> = Arc::new(ReqwestCalcClient {
                http: self.http.clone(),
                cfg: self.config.external_api.clone(),
                data_dir: self.config.server.data_dir.clone(),
            });

            let actor = CalcActor {
                id: calc_id.clone(),
                run_id: self.run_id.clone(),
                kind: kind.clone(),
                input_json: input_json.clone(),
                idempotency_key: idem_key.clone(),
                attempt: *attempt,
                max_attempts: *max_attempts,
                db: self.db.clone(),
                bus: self.bus.clone(),
                run_tx: notif_tx.clone(),
                pool: self.pool.clone(),
                api,
                retry_cfg: self.config.retry.clone(),
                lease_cfg: self.config.lease.clone(),
                data_dir: self.config.server.data_dir.clone(),
                worker_id: worker_id.clone(),
            };
            let calc_tx = actor.spawn();
            let _ = calc_tx.send(CalcCmd::Start).await;
            calc_txs.insert(calc_id.clone(), calc_tx);
        }

        let total = calc_txs.len();
        let mut finished = 0usize;

        loop {
            tokio::select! {
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(RunCmd::Cancel) | None => {
                            for tx in calc_txs.values() {
                                let _ = tx.send(CalcCmd::Cancel).await;
                            }
                            break;
                        }
                    }
                }
                notif = notif_rx.recv() => {
                    match notif {
                        Some(RunNotification::CalcFinished(id)) => {
                            calc_txs.remove(&id);
                            finished += 1;
                            self.refresh_run_status().await;
                            if finished >= total {
                                break;
                            }
                        }
                        Some(RunNotification::Cancel) | None => break,
                    }
                }
            }
        }

        info!(run_id = %self.run_id, "run actor finished");
    }

    async fn refresh_run_status(&self) {
        match self.db.get_calc_statuses_for_run(self.run_id.clone()).await {
            Ok(statuses) => {
                let new_status = derive_run_status(&statuses);
                let _ = self.db.update_run_status(self.run_id.clone(), new_status).await;
            }
            Err(e) => error!(run_id = %self.run_id, error = %e, "failed to refresh run status"),
        }
    }
}
