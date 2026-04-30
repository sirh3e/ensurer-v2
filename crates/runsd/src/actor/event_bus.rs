use common::event::SequencedEvent;
use tokio::sync::broadcast;

pub const EVENT_BUS_CAPACITY: usize = 1024;

/// Thin wrapper so the bus is easy to clone and pass around.
#[derive(Clone, Debug)]
pub struct EventBus {
    tx: broadcast::Sender<SequencedEvent>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(EVENT_BUS_CAPACITY);
        Self { tx }
    }

    pub fn publish(&self, event: SequencedEvent) {
        // Ignore SendError — it just means no subscribers at this moment.
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SequencedEvent> {
        self.tx.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
