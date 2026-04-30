use std::{
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use chrono::Utc;
use futures::Future;
use rand::Rng;
use tokio::{
    sync::{mpsc, oneshot},
    time,
};
use tracing::{error, info, warn};

use common::{
    event::{SequencedEvent, ServerEvent},
    model::{CalcStatus, ErrorKind},
    types::{CalcId, RunId},
};

use crate::{
    actor::{db::DbHandle, event_bus::EventBus, run::RunNotification, worker_pool::WorkerPool},
    config::{ExternalApiConfig, LeaseConfig, RetryConfig},
};

// ── External API call ─────────────────────────────────────────────────────────

/// Result of calling the external calculation API.
pub enum ApiOutcome {
    Succeeded { result_path: PathBuf },
    TransientError { message: String },
    PermanentError { message: String },
}

/// Thin wrapper around the external HTTP API. Keeping it behind a trait allows
/// test-doubles without a live server.
pub trait CalcApiClient: Send + Sync + 'static {
    fn submit<'a>(
        &'a self,
        kind: &'a str,
        input_json: &'a serde_json::Value,
        idempotency_key: &'a str,
        data_dir: &'a Path,
        calc_id: &'a CalcId,
    ) -> Pin<Box<dyn Future<Output = ApiOutcome> + Send + 'a>>;
}

/// Production implementation backed by reqwest.
pub struct ReqwestCalcClient {
    pub http: reqwest::Client,
    pub cfg: ExternalApiConfig,
    pub data_dir: PathBuf,
}

impl CalcApiClient for ReqwestCalcClient {
    fn submit<'a>(
        &'a self,
        kind: &'a str,
        input_json: &'a serde_json::Value,
        idempotency_key: &'a str,
        data_dir: &'a Path,
        calc_id: &'a CalcId,
    ) -> Pin<Box<dyn Future<Output = ApiOutcome> + Send + 'a>> {
        Box::pin(async move {
            // ── Phase 1: launch the task ──────────────────────────────────────
            let launch_url = format!("{}{}", self.cfg.base_url, self.cfg.launch_path);
            let mut launch_body = input_json.clone();
            if let Some(obj) = launch_body.as_object_mut() {
                obj.insert("kind".into(), serde_json::Value::String(kind.to_string()));
            }

            let mut req = self
                .http
                .post(&launch_url)
                .timeout(Duration::from_secs(self.cfg.request_timeout_s))
                .json(&launch_body);

            if self.cfg.supports_idempotency {
                req = req.header("Idempotency-Key", idempotency_key);
            }

            let task_id = match req.send().await {
                Err(e) => {
                    return ApiOutcome::TransientError {
                        message: e.to_string(),
                    };
                }
                Ok(resp) => {
                    let status = resp.status();
                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS
                        || status == reqwest::StatusCode::REQUEST_TIMEOUT
                        || status.is_server_error()
                    {
                        return ApiOutcome::TransientError {
                            message: format!("launch HTTP {}", status),
                        };
                    }
                    if !status.is_success() {
                        return ApiOutcome::PermanentError {
                            message: format!("launch HTTP {}", status),
                        };
                    }
                    match resp.json::<serde_json::Value>().await {
                        Err(e) => {
                            return ApiOutcome::TransientError {
                                message: format!("launch response parse error: {e}"),
                            };
                        }
                        Ok(body) => body
                            .get("taskId")
                            .or_else(|| body.get("task_id"))
                            .or_else(|| body.get("id"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_default(),
                    }
                }
            };

            // ── Phase 2: poll until the task finishes ─────────────────────────
            let status_url = format!("{}{}", self.cfg.base_url, self.cfg.status_path);
            let poll_interval = Duration::from_millis(self.cfg.poll_interval_ms);

            loop {
                tokio::time::sleep(poll_interval).await;

                let poll_resp = self
                    .http
                    .get(&status_url)
                    .query(&[("taskId", &task_id)])
                    .timeout(Duration::from_secs(self.cfg.request_timeout_s))
                    .send()
                    .await;

                match poll_resp {
                    Err(e) => {
                        return ApiOutcome::TransientError {
                            message: e.to_string(),
                        };
                    }
                    Ok(resp) => {
                        let http_status = resp.status();
                        if http_status == reqwest::StatusCode::TOO_MANY_REQUESTS
                            || http_status == reqwest::StatusCode::REQUEST_TIMEOUT
                            || http_status.is_server_error()
                        {
                            return ApiOutcome::TransientError {
                                message: format!("status HTTP {}", http_status),
                            };
                        }
                        if !http_status.is_success() {
                            return ApiOutcome::PermanentError {
                                message: format!("status HTTP {}", http_status),
                            };
                        }

                        let body = match resp.json::<serde_json::Value>().await {
                            Ok(b) => b,
                            Err(e) => {
                                return ApiOutcome::TransientError {
                                    message: format!("status response parse error: {e}"),
                                };
                            }
                        };

                        let task_status = body
                            .get("status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_lowercase();

                        match task_status.as_str() {
                            "completed" | "succeeded" | "success" => {
                                let result_dir =
                                    data_dir.join("results").join(calc_id.0.to_string());
                                let _ = tokio::fs::create_dir_all(&result_dir).await;
                                let result_path = result_dir.join("result.json");
                                let bytes = serde_json::to_vec(&body).unwrap_or_default();
                                if tokio::fs::write(&result_path, &bytes).await.is_err() {
                                    return ApiOutcome::TransientError {
                                        message: "failed to write result to disk".into(),
                                    };
                                }
                                return ApiOutcome::Succeeded { result_path };
                            }
                            "failed" | "error" => {
                                let msg = body
                                    .get("error")
                                    .or_else(|| body.get("message"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("task failed")
                                    .to_string();
                                return ApiOutcome::PermanentError { message: msg };
                            }
                            // "running", "pending", "" → keep polling
                            _ => continue,
                        }
                    }
                }
            }
        })
    }
}

// ── Commands ──────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum CalcCmd {
    Start,
    Cancel,
    HeartbeatTick,
}

// ── Actor ─────────────────────────────────────────────────────────────────────

pub struct CalcActor {
    pub id: CalcId,
    pub run_id: RunId,
    pub kind: String,
    pub input_json: serde_json::Value,
    pub idempotency_key: String,
    pub attempt: u32,
    pub max_attempts: u32,
    pub db: DbHandle,
    pub bus: EventBus,
    pub run_tx: mpsc::Sender<RunNotification>,
    pub pool: WorkerPool,
    pub api: Arc<dyn CalcApiClient>,
    pub retry_cfg: RetryConfig,
    pub lease_cfg: LeaseConfig,
    pub data_dir: PathBuf,
    pub worker_id: String,
}

impl CalcActor {
    pub fn spawn(self) -> mpsc::Sender<CalcCmd> {
        let (tx, rx) = mpsc::channel(8);
        tokio::spawn(async move { self.run(rx).await });
        tx
    }

    async fn run(mut self, mut rx: mpsc::Receiver<CalcCmd>) {
        loop {
            match rx.recv().await {
                None => break,
                Some(CalcCmd::Cancel) => {
                    self.handle_cancel().await;
                    break;
                }
                Some(CalcCmd::HeartbeatTick) => {}
                Some(CalcCmd::Start) => {
                    let _permit = match self.pool.acquire().await {
                        Ok(p) => p,
                        Err(_) => break,
                    };

                    let (_cancel_tx, cancel_rx) = oneshot::channel::<()>();

                    // Heartbeat task.
                    let hb_id = self.id.clone();
                    let hb_db = self.db.clone();
                    let hb_expiry = self.lease_cfg.expiry_s;
                    let hb_interval = self.lease_cfg.heartbeat_interval_s;
                    let hb = tokio::spawn(async move {
                        let mut interval = time::interval(Duration::from_secs(hb_interval));
                        loop {
                            interval.tick().await;
                            let expires =
                                Utc::now().timestamp_millis() + (hb_expiry as i64 * 1_000);
                            if hb_db.calc_heartbeat(hb_id.clone(), expires).await.is_err() {
                                break;
                            }
                        }
                    });

                    let expires_at =
                        Utc::now().timestamp_millis() + (self.lease_cfg.expiry_s as i64 * 1_000);
                    if let Err(e) = self
                        .db
                        .calc_started(self.id.clone(), self.worker_id.clone(), expires_at)
                        .await
                    {
                        error!(calc_id = %self.id, error = %e, "failed to acquire lease");
                        hb.abort();
                        break;
                    }

                    self.emit_status(CalcStatus::Pending, CalcStatus::Running)
                        .await;

                    let api = Arc::clone(&self.api);
                    let kind = self.kind.clone();
                    let input = self.input_json.clone();
                    let idem = self.idempotency_key.clone();
                    let data_dir = self.data_dir.clone();
                    let calc_id = self.id.clone();

                    let outcome = tokio::select! {
                        o = api.submit(&kind, &input, &idem, &data_dir, &calc_id) => o,
                        _ = cancel_rx => {
                            hb.abort();
                            self.handle_cancel().await;
                            break;
                        }
                    };

                    hb.abort();

                    match outcome {
                        ApiOutcome::Succeeded { result_path } => {
                            self.handle_success(result_path).await;
                            break;
                        }
                        ApiOutcome::TransientError { message } => {
                            if self.handle_transient_error(message, &mut rx).await {
                                break;
                            }
                            // Else re-loop — synthetic restart.
                        }
                        ApiOutcome::PermanentError { message } => {
                            self.handle_permanent_error(message).await;
                            break;
                        }
                    }
                }
            }
        }
    }

    async fn handle_success(&self, result_path: PathBuf) {
        let path_str = result_path.to_string_lossy().to_string();
        let _ = self.db.calc_succeeded(self.id.clone(), path_str).await;
        self.emit_status(CalcStatus::Running, CalcStatus::Succeeded)
            .await;
        let _ = self
            .run_tx
            .send(RunNotification::CalcFinished(self.id.clone()))
            .await;
        info!(calc_id = %self.id, "calculation succeeded");
    }

    async fn handle_permanent_error(&self, message: String) {
        let _ = self
            .db
            .calc_failed(
                self.id.clone(),
                ErrorKind::Permanent.to_string(),
                message.clone(),
            )
            .await;
        self.emit_status(CalcStatus::Running, CalcStatus::Failed)
            .await;
        let _ = self
            .run_tx
            .send(RunNotification::CalcFinished(self.id.clone()))
            .await;
        warn!(calc_id = %self.id, error = %message, "calculation permanently failed");
    }

    /// Returns true if the actor should terminate (exhausted or cancelled).
    async fn handle_transient_error(
        &mut self,
        message: String,
        rx: &mut mpsc::Receiver<CalcCmd>,
    ) -> bool {
        self.attempt += 1;
        if self.attempt > self.max_attempts {
            let _ = self
                .db
                .calc_failed(
                    self.id.clone(),
                    ErrorKind::TransientExhausted.to_string(),
                    message,
                )
                .await;
            self.emit_status(CalcStatus::Running, CalcStatus::Failed)
                .await;
            let _ = self
                .run_tx
                .send(RunNotification::CalcFinished(self.id.clone()))
                .await;
            return true;
        }

        let delay = jittered_backoff(
            self.attempt,
            self.retry_cfg.base_delay_ms,
            self.retry_cfg.max_delay_ms,
        );
        let next_at = Utc::now().timestamp_millis() + delay as i64;
        let _ = self
            .db
            .calc_retrying(self.id.clone(), self.attempt, next_at)
            .await;
        self.emit_status(CalcStatus::Running, CalcStatus::Retrying)
            .await;

        tokio::select! {
            _ = time::sleep(Duration::from_millis(delay)) => {}
            msg = rx.recv() => {
                if matches!(msg, Some(CalcCmd::Cancel) | None) {
                    self.handle_cancel().await;
                    return true;
                }
            }
        }

        // Back to pending for a fresh Start loop iteration.
        let _ = self.db.calc_reset_pending(self.id.clone()).await;
        false
    }

    async fn handle_cancel(&self) {
        let _ = self.db.calc_cancelled(self.id.clone()).await;
        self.emit_status(CalcStatus::Running, CalcStatus::Cancelled)
            .await;
        let _ = self
            .run_tx
            .send(RunNotification::CalcFinished(self.id.clone()))
            .await;
        info!(calc_id = %self.id, "calculation cancelled");
    }

    async fn emit_status(&self, from: CalcStatus, to: CalcStatus) {
        let at = Utc::now();
        let event = ServerEvent::CalcStatusChanged {
            run_id: self.run_id.clone(),
            calculation_id: self.id.clone(),
            from,
            to,
            attempt: self.attempt,
            at,
        };
        let payload = serde_json::to_string(&event).unwrap_or_default();
        let seq = self
            .db
            .insert_event(
                Some(self.run_id.to_string()),
                Some(self.id.to_string()),
                event.event_kind_str().to_string(),
                payload,
            )
            .await
            .unwrap_or(0);
        self.bus.publish(SequencedEvent { seq, event });
    }
}

fn jittered_backoff(attempt: u32, base_ms: u64, max_ms: u64) -> u64 {
    let exp = (base_ms as f64) * (2_f64.powi(attempt as i32 - 1));
    let capped = exp.min(max_ms as f64) as u64;
    rand::thread_rng().gen_range(0..=capped)
}
