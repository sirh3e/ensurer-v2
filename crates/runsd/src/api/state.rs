use std::sync::Arc;

use crate::{
    actor::{db::DbHandle, event_bus::EventBus, supervisor::SupervisorHandle},
    config::Config,
    db::queries::Db,
};

#[derive(Clone)]
pub struct AppState {
    pub db: DbHandle,
    pub read_pool: Db,
    pub bus: EventBus,
    pub supervisor: SupervisorHandle,
    pub config: Arc<Config>,
}
