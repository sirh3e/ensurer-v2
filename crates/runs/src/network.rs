pub mod sse;
pub mod unix_client;

pub use unix_client::Client;

use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{
    event::SequencedEvent,
    model::{NewCalc, SubmitRunRequest},
    types::{CalcId, RunId},
};
use tokio::sync::mpsc;
use tracing::warn;

use crate::app::msg::AppMsg;

/// Commands the main loop sends to the network task.
#[derive(Debug)]
pub enum NetworkCmd {
    SubmitRun {
        jira_issue_id: String,
        calcs: Vec<NewCalc>,
    },
    CancelRun {
        run_id: RunId,
    },
    CancelCalc {
        run_id: RunId,
        calc_id: CalcId,
    },
    RetryCalc {
        run_id: RunId,
        calc_id: CalcId,
    },
    /// Recursively import all *.json files from `path` as run submissions.
    ImportDirectory {
        path: PathBuf,
    },
    /// Load the next page of runs using the cursor returned from a previous response.
    LoadMoreRuns {
        cursor: String,
    },
    /// Re-fetch the full run list from scratch.
    RefreshRuns,
    /// Fetch a single run by ID (triggered by RunSubmitted SSE event).
    FetchRun {
        run_id: RunId,
    },
}

/// Spawn the network task. Returns a sender for NetworkCmds.
#[cfg(unix)]
pub fn spawn(
    socket_path: PathBuf,
    app_tx: mpsc::Sender<AppMsg>,
    page_size: u32,
) -> mpsc::Sender<NetworkCmd> {
    let (cmd_tx, cmd_rx) = mpsc::channel::<NetworkCmd>(32);
    tokio::spawn(async move {
        run_network(Client::new(socket_path), app_tx, cmd_rx, page_size).await;
    });
    cmd_tx
}

#[cfg(windows)]
pub fn spawn(
    port: u16,
    app_tx: mpsc::Sender<AppMsg>,
    page_size: u32,
) -> mpsc::Sender<NetworkCmd> {
    let (cmd_tx, cmd_rx) = mpsc::channel::<NetworkCmd>(32);
    tokio::spawn(async move {
        run_network(Client::new(port), app_tx, cmd_rx, page_size).await;
    });
    cmd_tx
}

async fn run_network(
    client: Client,
    app_tx: mpsc::Sender<AppMsg>,
    mut cmd_rx: mpsc::Receiver<NetworkCmd>,
    page_size: u32,
) {
    // Initial fetch.
    match client
        .get_json::<common::model::ListRunsResponse>(&format!("/runs?limit={page_size}"))
        .await
    {
        Ok(body) => {
            let _ = app_tx.send(AppMsg::RunsLoaded(body.runs, body.next_cursor)).await;
        }
        Err(e) => {
            let _ = app_tx.send(AppMsg::CmdErr(format!("initial fetch failed: {e}"))).await;
        }
    }

    // SSE reconnect loop with exponential backoff (500 ms → 30 s).
    let sse_app_tx = app_tx.clone();
    let sse_client = client.clone();
    tokio::spawn(async move {
        let mut last_seq: i64 = 0;
        let mut backoff_ms: u64 = 500;
        const MAX_BACKOFF_MS: u64 = 30_000;

        loop {
            let _ = sse_app_tx.send(AppMsg::SseDisconnected).await;
            match sse_client.sse_connect(&format!("/events?since={last_seq}")).await {
                Ok(body) => {
                    let _ = sse_app_tx.send(AppMsg::SseReconnected).await;
                    backoff_ms = 500; // reset on successful connect
                    let mut stream = sse::SseStream::new(body);
                    while let Some(result) = stream.next().await {
                        match result {
                            Ok(event) => {
                                if let Some(data) = event.data {
                                    match serde_json::from_str::<SequencedEvent>(&data) {
                                        Ok(seq_event) => {
                                            last_seq = seq_event.seq;
                                            let _ = sse_app_tx.send(AppMsg::ServerEvent(seq_event)).await;
                                        }
                                        Err(e) => {
                                            warn!(error = %e, data = %data, "SSE JSON parse error");
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                warn!(error = %e, "SSE stream error");
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, backoff_ms, "SSE connect failed");
                }
            }
            tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).min(MAX_BACKOFF_MS);
        }
    });

    // Command loop.
    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            NetworkCmd::LoadMoreRuns { cursor } => {
            let url = format!("/runs?limit=100&cursor={cursor}");
            match client.get_json::<common::model::ListRunsResponse>(&url).await {
                Ok(body) => {
                    let _ = app_tx.send(AppMsg::MoreRunsLoaded(body.runs, body.next_cursor)).await;
                }
                Err(e) => {
                    let _ = app_tx.send(AppMsg::CmdErr(format!("load more failed: {e}"))).await;
                }
            }
        }
        NetworkCmd::RefreshRuns => {
            match client
                .get_json::<common::model::ListRunsResponse>(&format!("/runs?limit={page_size}"))
                .await
            {
                Ok(body) => {
                    let _ = app_tx.send(AppMsg::RunsLoaded(body.runs, body.next_cursor)).await;
                }
                Err(e) => {
                    let _ = app_tx.send(AppMsg::CmdErr(format!("refresh failed: {e}"))).await;
                }
            }
        }
        NetworkCmd::FetchRun { run_id } => {
            match client
                .get_json::<common::model::Run>(&format!("/runs/{run_id}"))
                .await
            {
                Ok(run) => {
                    let _ = app_tx.send(AppMsg::RunFetched(run)).await;
                }
                Err(e) => {
                    warn!(run_id = %run_id, error = %e, "failed to fetch run after RunSubmitted");
                }
            }
        }
        NetworkCmd::ImportDirectory { path } => {
                // Run import in background so other commands aren't blocked.
                let imp_client = client.clone();
                let imp_tx = app_tx.clone();
                tokio::spawn(async move {
                    import_directory(imp_client, path, imp_tx).await;
                });
            }
            other => {
                let result = handle_cmd(&client, other).await;
                match result {
                    Ok(msg) => { let _ = app_tx.send(AppMsg::CmdOk(msg)).await; }
                    Err(e)  => { let _ = app_tx.send(AppMsg::CmdErr(e)).await; }
                }
            }
        }
    }
}

async fn handle_cmd(client: &Client, cmd: NetworkCmd) -> Result<String, String> {
    match cmd {
        NetworkCmd::SubmitRun { jira_issue_id, calcs } => {
            let body = SubmitRunRequest { jira_issue_id, calculations: calcs };
            let status = client.post_json("/runs", &body).await.map_err(|e| e.to_string())?;
            if status.is_success() {
                Ok("Run submitted".into())
            } else {
                Err(format!("Submit failed: HTTP {status}"))
            }
        }
        NetworkCmd::CancelRun { run_id } => {
            client.post_empty(&format!("/runs/{run_id}/cancel")).await.map_err(|e| e.to_string())?;
            Ok("Run cancelled".into())
        }
        NetworkCmd::CancelCalc { calc_id, .. } => {
            client.post_empty(&format!("/calculations/{calc_id}/cancel")).await.map_err(|e| e.to_string())?;
            Ok("Calculation cancelled".into())
        }
        NetworkCmd::RetryCalc { calc_id, .. } => {
            client.post_empty(&format!("/calculations/{calc_id}/retry")).await.map_err(|e| e.to_string())?;
            Ok("Retry scheduled".into())
        }
        // These are handled in the command loop above, never reach handle_cmd.
        NetworkCmd::ImportDirectory { .. }
        | NetworkCmd::LoadMoreRuns { .. }
        | NetworkCmd::RefreshRuns
        | NetworkCmd::FetchRun { .. } => unreachable!(),
    }
}

// ── JSON import ───────────────────────────────────────────────────────────────

async fn import_directory(client: Client, dir: PathBuf, app_tx: mpsc::Sender<AppMsg>) {
    let files = collect_json_files(&dir);
    let total = files.len();

    if total == 0 {
        let _ = app_tx.send(AppMsg::CmdErr(format!("no JSON files found in {}", dir.display()))).await;
        return;
    }

    let _ = app_tx.send(AppMsg::ImportProgress { done: 0, total, errors: 0 }).await;

    let mut submitted = 0usize;
    let mut errors = 0usize;

    for path in &files {
        match import_file(&client, path).await {
            Ok(()) => submitted += 1,
            Err(e) => {
                errors += 1;
                warn!(path = %path.display(), error = %e, "import file failed");
            }
        }
        let _ = app_tx.send(AppMsg::ImportProgress { done: submitted + errors, total, errors }).await;
    }

    if errors == 0 {
        let _ = app_tx.send(AppMsg::CmdOk(format!("Imported {submitted}/{total} runs"))).await;
    } else {
        let _ = app_tx.send(AppMsg::CmdErr(format!(
            "Import done: {submitted} ok, {errors} failed (check logs)"
        ))).await;
    }
}

async fn import_file(client: &Client, path: &Path) -> Result<(), String> {
    let raw = tokio::fs::read(path).await.map_err(|e| e.to_string())?;
    let req: SubmitRunRequest = serde_json::from_slice(&raw)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let status = client.post_json("/runs", &req).await.map_err(|e| e.to_string())?;
    if status.is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {status}"))
    }
}

/// Recursively collect all `*.json` files under `dir`.
fn collect_json_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(collect_json_files(&path));
            } else if path.extension().is_some_and(|e| e == "json") {
                out.push(path);
            }
        }
    }
    out
}
