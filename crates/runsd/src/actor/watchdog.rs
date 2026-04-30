use std::time::Duration;

use chrono::Utc;
use tokio::time;
use tracing::warn;

use crate::{
    actor::{db::DbHandle, supervisor::SupervisorHandle},
    config::LeaseConfig,
};

/// Periodically scans for expired leases and schedules retries.
pub async fn run_watchdog(db: DbHandle, supervisor: SupervisorHandle, cfg: LeaseConfig) {
    let mut interval = time::interval(Duration::from_secs(cfg.watchdog_interval_s));
    loop {
        interval.tick().await;
        let now = Utc::now().timestamp_millis();

        // Expired running leases — mark as retrying or failed.
        match db.list_expired_leases(now).await {
            Err(e) => {
                warn!(error = %e, "watchdog: failed to list expired leases");
            }
            Ok(calcs) => {
                for calc in calcs {
                    let next_attempt = calc.attempt + 1;
                    if next_attempt > calc.max_attempts {
                        if let Err(e) = db
                            .calc_failed(
                                calc.id.clone(),
                                "crash_exhausted".into(),
                                "lease expired; attempts exhausted".into(),
                            )
                            .await
                        {
                            warn!(calc_id = %calc.id, error = %e, "watchdog: failed to mark calc failed");
                        }
                    } else {
                        let delay_ms = 5_000_u64 * next_attempt as u64;
                        let next_at = now + delay_ms as i64;
                        if let Err(e) = db
                            .calc_retrying(calc.id.clone(), next_attempt, next_at)
                            .await
                        {
                            warn!(calc_id = %calc.id, error = %e, "watchdog: failed to set calc retrying");
                        } else {
                            supervisor.reschedule_calc(calc.run_id.clone(), calc.id.clone());
                        }
                    }
                }
            }
        }

        // Ready retries — reschedule via supervisor.
        match db.list_ready_retries(now).await {
            Err(e) => {
                warn!(error = %e, "watchdog: failed to list ready retries");
            }
            Ok(calcs) => {
                for calc in calcs {
                    supervisor.reschedule_calc(calc.run_id.clone(), calc.id.clone());
                }
            }
        }
    }
}
