use common::{event::SequencedEvent, model::Run};
use crossterm::event::KeyEvent;

/// All messages that flow into the central update function.
#[derive(Debug)]
pub enum AppMsg {
    /// A key was pressed.
    Key(KeyEvent),
    /// The SSE stream delivered a new server event.
    ServerEvent(SequencedEvent),
    /// Initial run list fetched on startup (replaces existing list).
    RunsLoaded(Vec<Run>, Option<String>),
    /// Next page of runs appended to the existing list.
    MoreRunsLoaded(Vec<Run>, Option<String>),
    /// A single run fetched in response to a RunSubmitted SSE event.
    RunFetched(Run),
    /// A command (submit/cancel/retry) completed successfully.
    CmdOk(String),
    /// A command failed.
    CmdErr(String),
    /// SSE connection dropped; showing reconnecting state.
    SseDisconnected,
    /// SSE reconnected.
    SseReconnected,
    /// Terminal was resized.
    Resize(u16, u16),
    /// Progress update from a running directory import.
    ImportProgress { done: usize, total: usize, errors: usize },
    /// The render loop should quit.
    Quit,
    /// Fired every render frame when no other message is pending; drives the spinner.
    Tick,
}
