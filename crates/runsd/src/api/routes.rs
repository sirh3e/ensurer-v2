use axum::{
    Extension, Router,
    routing::{get, post},
};

use super::{
    graphql::{build_schema, graphql_handler, graphql_playground},
    handlers::*,
    sse::sse_handler,
    state::AppState,
};

pub fn router(state: AppState) -> Router {
    let schema = build_schema(state.clone());

    Router::new()
        .route("/healthz", get(healthz))
        .route("/metrics", get(metrics))
        .route("/runs", post(submit_run).get(list_runs))
        .route("/runs/{id}", get(get_run))
        .route("/runs/{id}/cancel", post(cancel_run))
        .route("/calculations/{id}", get(get_calculation))
        .route("/calculations/{id}/retry", post(retry_calculation))
        .route("/calculations/{id}/cancel", post(cancel_calculation))
        .route("/calculations/{id}/result", get(get_result))
        .route("/events", get(sse_handler))
        .route("/graphql", post(graphql_handler).get(graphql_playground))
        .layer(Extension(schema))
        .with_state(state)
}
