use axum::{
    extract::{Query, State},
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::stream::Stream;
use serde::Deserialize;
use std::{convert::Infallible, time::Duration};
use tokio::sync::broadcast::error::RecvError;
use tracing::warn;

use crate::{api::state::AppState, db::queries};

#[derive(Debug, Deserialize)]
pub struct SseQuery {
    pub since: Option<i64>,
}

/// GET /events?since={seq}
/// Replays historical events from the DB then switches to live push.
pub async fn sse_handler(
    State(state): State<AppState>,
    Query(query): Query<SseQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let since = query.since.unwrap_or(0);
    let bus = state.bus.clone();
    let read_pool = state.read_pool.clone();

    let stream = async_stream::stream! {
        // 1. Replay historical events from DB.
        match queries::list_events_since(&read_pool, since, 1000).await {
            Ok(rows) => {
                for row in rows {
                    let event = Event::default()
                        .id(row.seq.to_string())
                        .event(row.kind.clone())
                        .data(row.payload_json.clone());
                    yield Ok(event);
                }
            }
            Err(e) => {
                warn!(error = %e, "SSE: failed to replay events from DB");
            }
        }

        // 2. Subscribe to live events.
        let mut sub = bus.subscribe();
        loop {
            match sub.recv().await {
                Ok(sequenced) => {
                    let data = match serde_json::to_string(&sequenced.event) {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    let event = Event::default()
                        .id(sequenced.seq.to_string())
                        .event(sequenced.event.event_kind_str())
                        .data(data);
                    yield Ok(event);
                }
                Err(RecvError::Lagged(n)) => {
                    warn!(missed = n, "SSE client lagged; client should reconnect with since=");
                    // Emit a comment so the client knows it missed events.
                    yield Ok(Event::default().comment(format!("lagged:{n}")));
                }
                Err(RecvError::Closed) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}
