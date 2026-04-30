use std::sync::Arc;
use tokio::sync::{Semaphore, OwnedSemaphorePermit};

/// Bounded pool of worker permits. Calculation Actors acquire one permit before
/// doing heavy work; dropping the permit releases it back to the pool.
#[derive(Clone, Debug)]
pub struct WorkerPool {
    semaphore: Arc<Semaphore>,
}

impl WorkerPool {
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
        }
    }

    /// Acquire a permit, waiting until one is available.
    pub async fn acquire(&self) -> Result<OwnedSemaphorePermit, tokio::sync::AcquireError> {
        Arc::clone(&self.semaphore).acquire_owned().await
    }
}
