use async_graphql::{
    Context, EmptySubscription, Enum, InputObject, Object, Result as GqlResult, Schema,
    SimpleObject,
};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{Extension, response::IntoResponse};

use common::{
    model::{CalcStatus, NewCalc, RunStatus},
    types::{CalcId, RunId},
};

use crate::api::state::AppState;

// ── Status enums ──────────────────────────────────────────────────────────────

#[derive(Enum, Copy, Clone, PartialEq, Eq)]
pub enum RunStatusGql {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    PartiallySucceeded,
}

impl From<RunStatus> for RunStatusGql {
    fn from(s: RunStatus) -> Self {
        match s {
            RunStatus::Pending => Self::Pending,
            RunStatus::Running => Self::Running,
            RunStatus::Succeeded => Self::Succeeded,
            RunStatus::Failed => Self::Failed,
            RunStatus::Cancelled => Self::Cancelled,
            RunStatus::PartiallySucceeded => Self::PartiallySucceeded,
        }
    }
}

#[derive(Enum, Copy, Clone, PartialEq, Eq)]
pub enum CalcStatusGql {
    Pending,
    Running,
    Retrying,
    Succeeded,
    Failed,
    Cancelled,
}

impl From<CalcStatus> for CalcStatusGql {
    fn from(s: CalcStatus) -> Self {
        match s {
            CalcStatus::Pending => Self::Pending,
            CalcStatus::Running => Self::Running,
            CalcStatus::Retrying => Self::Retrying,
            CalcStatus::Succeeded => Self::Succeeded,
            CalcStatus::Failed => Self::Failed,
            CalcStatus::Cancelled => Self::Cancelled,
        }
    }
}

// ── Output types ──────────────────────────────────────────────────────────────

#[derive(SimpleObject)]
pub struct CalcGql {
    pub id: String,
    pub run_id: String,
    pub kind: String,
    /// JSON-encoded input payload.
    pub input_json: String,
    pub idempotency_key: String,
    pub status: CalcStatusGql,
    pub attempt: i32,
    pub max_attempts: i32,
    pub error_kind: Option<String>,
    pub error_message: Option<String>,
    pub result_path: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub updated_at: String,
}

impl From<common::model::Calculation> for CalcGql {
    fn from(c: common::model::Calculation) -> Self {
        Self {
            id: c.id.to_string(),
            run_id: c.run_id.to_string(),
            kind: c.kind,
            input_json: serde_json::to_string(&c.input_json)
                .expect("serializing a JSON Value is infallible"),
            idempotency_key: c.idempotency_key,
            status: c.status.into(),
            attempt: c.attempt as i32,
            max_attempts: c.max_attempts as i32,
            error_kind: c.error_kind.map(|e| e.to_string()),
            error_message: c.error_message,
            result_path: c.result_path,
            created_at: c.created_at.to_rfc3339(),
            started_at: c.started_at.map(|t| t.to_rfc3339()),
            completed_at: c.completed_at.map(|t| t.to_rfc3339()),
            updated_at: c.updated_at.to_rfc3339(),
        }
    }
}

#[derive(SimpleObject)]
pub struct RunGql {
    pub id: String,
    pub jira_issue_id: String,
    pub submitted_by: String,
    pub status: RunStatusGql,
    pub created_at: String,
    pub updated_at: String,
    pub calculations: Vec<CalcGql>,
}

impl From<common::model::Run> for RunGql {
    fn from(r: common::model::Run) -> Self {
        Self {
            id: r.id.to_string(),
            jira_issue_id: r.jira_issue_id,
            submitted_by: r.submitted_by,
            status: r.status.into(),
            created_at: r.created_at.to_rfc3339(),
            updated_at: r.updated_at.to_rfc3339(),
            calculations: r.calculations.into_iter().map(CalcGql::from).collect(),
        }
    }
}

// ── Input types ───────────────────────────────────────────────────────────────

#[derive(InputObject)]
pub struct NewCalcInput {
    pub kind: String,
    /// Optional JSON-encoded input (defaults to `{}`).
    pub input: Option<String>,
}

// ── Query root ────────────────────────────────────────────────────────────────

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    /// Fetch a single run by its UUID.
    async fn run(&self, ctx: &Context<'_>, id: String) -> GqlResult<Option<RunGql>> {
        let state = ctx.data::<AppState>()?;
        let run_id: RunId = id
            .parse()
            .map_err(|e| async_graphql::Error::new(format!("invalid run id: {e}")))?;
        let run = state
            .db
            .get_run(run_id)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(run.map(RunGql::from))
    }

    /// List runs, optionally filtered by status.
    async fn runs(
        &self,
        ctx: &Context<'_>,
        status: Option<String>,
        limit: Option<i32>,
    ) -> GqlResult<Vec<RunGql>> {
        let state = ctx.data::<AppState>()?;
        let limit = limit.unwrap_or(20).clamp(1, 100) as u32;
        let runs = state
            .db
            .list_runs(status, limit, None, None)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(runs.into_iter().map(RunGql::from).collect())
    }

    /// Fetch a single calculation by its UUID.
    async fn calculation(&self, ctx: &Context<'_>, id: String) -> GqlResult<Option<CalcGql>> {
        let state = ctx.data::<AppState>()?;
        let calc_id: CalcId = id
            .parse()
            .map_err(|e| async_graphql::Error::new(format!("invalid calc id: {e}")))?;
        let calc = state
            .db
            .get_calculation(calc_id)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(calc.map(CalcGql::from))
    }
}

// ── Mutation root ─────────────────────────────────────────────────────────────

pub struct MutationRoot;

#[Object]
impl MutationRoot {
    /// Submit a new run. Returns the new run's UUID.
    async fn submit_run(
        &self,
        ctx: &Context<'_>,
        jira_issue_id: String,
        calculations: Vec<NewCalcInput>,
    ) -> GqlResult<String> {
        let state = ctx.data::<AppState>()?;

        if jira_issue_id.trim().is_empty() {
            return Err(async_graphql::Error::new("jiraIssueId must not be empty"));
        }
        if calculations.is_empty() {
            return Err(async_graphql::Error::new(
                "at least one calculation is required",
            ));
        }

        let calcs = calculations
            .into_iter()
            .enumerate()
            .map(|(i, nc)| {
                if nc.kind.trim().is_empty() {
                    return Err(async_graphql::Error::new(format!(
                        "calculations[{i}].kind must not be empty"
                    )));
                }
                let input: serde_json::Value = nc
                    .input
                    .as_deref()
                    .map(|s| {
                        serde_json::from_str(s)
                            .map_err(|e| async_graphql::Error::new(format!("invalid JSON: {e}")))
                    })
                    .transpose()?
                    .unwrap_or(serde_json::Value::Object(Default::default()));
                Ok(NewCalc {
                    kind: nc.kind,
                    input,
                })
            })
            .collect::<Result<Vec<_>, async_graphql::Error>>()?;

        let run_id = state
            .supervisor
            .submit_run(jira_issue_id, "anonymous".into(), calcs)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(run_id.to_string())
    }

    /// Cancel all pending/running calculations in a run.
    async fn cancel_run(&self, ctx: &Context<'_>, id: String) -> GqlResult<bool> {
        let state = ctx.data::<AppState>()?;
        let run_id: RunId = id
            .parse()
            .map_err(|e| async_graphql::Error::new(format!("invalid run id: {e}")))?;
        state
            .supervisor
            .cancel_run(run_id)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(true)
    }

    /// Retry a failed calculation from the beginning.
    async fn retry_calculation(&self, ctx: &Context<'_>, id: String) -> GqlResult<bool> {
        let state = ctx.data::<AppState>()?;
        let calc_id: CalcId = id
            .parse()
            .map_err(|e| async_graphql::Error::new(format!("invalid calc id: {e}")))?;
        let calc = state
            .db
            .get_calculation(calc_id.clone())
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?
            .ok_or_else(|| async_graphql::Error::new("calculation not found"))?;
        if calc.status != CalcStatus::Failed {
            return Err(async_graphql::Error::new(
                "only calculations in 'failed' status can be retried",
            ));
        }
        state
            .supervisor
            .retry_calc(calc.run_id, calc_id)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(true)
    }

    /// Cancel a specific calculation.
    async fn cancel_calculation(&self, ctx: &Context<'_>, id: String) -> GqlResult<bool> {
        let state = ctx.data::<AppState>()?;
        let calc_id: CalcId = id
            .parse()
            .map_err(|e| async_graphql::Error::new(format!("invalid calc id: {e}")))?;
        let calc = state
            .db
            .get_calculation(calc_id.clone())
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?
            .ok_or_else(|| async_graphql::Error::new("calculation not found"))?;
        state
            .supervisor
            .cancel_calc(calc.run_id, calc_id)
            .await
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        Ok(true)
    }
}

// ── Schema & handlers ─────────────────────────────────────────────────────────

pub type AppSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;

pub fn build_schema(state: AppState) -> AppSchema {
    Schema::build(QueryRoot, MutationRoot, EmptySubscription)
        .data(state)
        .finish()
}

pub async fn graphql_handler(
    Extension(schema): Extension<AppSchema>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    schema.execute(req.into_inner()).await.into()
}

pub async fn graphql_playground() -> impl IntoResponse {
    axum::response::Html(async_graphql::http::playground_source(
        async_graphql::http::GraphQLPlaygroundConfig::new("/graphql"),
    ))
}
