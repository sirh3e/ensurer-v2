use common::{
    model::{CalcStatus, Calculation, Run},
    types::{CalcId, RunId},
};
use std::collections::HashMap;

// ── Pane / focus ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    RunList,
    CalcList,
    Detail,
}

// ── Screen / overlay ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    RunList,
    RunDetail(RunId),
    CalcDetail(CalcId),
    Dashboard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    None,
    Filter,
    Help,
    Command(String),
    Confirm(ConfirmDialog),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmDialog {
    pub message: String,
    pub action: ConfirmAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    Quit,
    CancelRun(RunId),
    CancelCalc(CalcId),
    RetryCalc(CalcId),
}

// ── Filter state ──────────────────────────────────────────────────────────────

pub const FILTER_STATUSES: &[&str] =
    &["all", "pending", "running", "retrying", "succeeded", "failed", "cancelled"];

#[derive(Debug, Clone, Default)]
pub struct FilterState {
    pub status: Option<String>,
    pub search: Option<String>,
    pub search_matches: Vec<usize>, // indices into runs vec
    pub search_pos: usize,
    /// Cursor position inside the filter overlay list.
    pub filter_cursor: usize,
}

// ── App ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct App {
    pub runs: Vec<Run>,
    pub run_index: HashMap<RunId, usize>,
    pub screen: Screen,
    pub pane: Pane,
    pub run_cursor: usize,
    pub calc_cursor: usize,
    pub overlay: Overlay,
    pub filter: FilterState,
    pub status_bar: String,
    pub sse_connected: bool,
    pub last_seen_seq: i64,
    /// Scroll offset for the detail/JSON pane.
    pub detail_scroll: usize,
    /// Vim count accumulator ("5j" → count_buf = "5").
    pub count_buf: String,
    /// Cursor returned from the last list response; Some = more pages available.
    pub next_cursor: Option<String>,
    /// True while an initial fetch or manual refresh is in flight.
    pub loading: bool,
    /// Frame counter incremented on every Tick; drives spinner animation.
    pub tick: u64,
    /// Visual mode anchor row index (None = not in visual mode).
    pub visual_anchor: Option<usize>,
}

impl App {
    pub fn new() -> Self {
        Self {
            runs: Vec::new(),
            run_index: HashMap::new(),
            screen: Screen::RunList,
            pane: Pane::RunList,
            run_cursor: 0,
            calc_cursor: 0,
            overlay: Overlay::None,
            filter: FilterState::default(),
            status_bar: "Loading…".to_string(),
            sse_connected: false,
            last_seen_seq: 0,
            detail_scroll: 0,
            count_buf: String::new(),
            next_cursor: None,
            loading: true,
            tick: 0,
            visual_anchor: None,
        }
    }

    pub fn visible_runs(&self) -> Vec<&Run> {
        self.runs
            .iter()
            .filter(|r| {
                let status_ok = self
                    .filter
                    .status
                    .as_ref()
                    .map(|s| r.status.to_string() == *s)
                    .unwrap_or(true);
                let search_ok = self
                    .filter
                    .search
                    .as_ref()
                    .map(|q| {
                        r.jira_issue_id.contains(q.as_str())
                            || r.calculations
                                .iter()
                                .any(|c| c.kind.contains(q.as_str()))
                    })
                    .unwrap_or(true);
                status_ok && search_ok
            })
            .collect()
    }

    pub fn selected_run(&self) -> Option<&Run> {
        self.visible_runs().get(self.run_cursor).copied()
    }

    pub fn selected_calc(&self) -> Option<&Calculation> {
        let run = self.selected_run()?;
        run.calculations.get(self.calc_cursor)
    }

    pub fn upsert_run(&mut self, run: Run) {
        if let Some(&idx) = self.run_index.get(&run.id) {
            self.runs[idx] = run;
        } else {
            let idx = self.runs.len();
            self.run_index.insert(run.id.clone(), idx);
            self.runs.push(run);
        }
    }

    pub fn update_calc_status(&mut self, run_id: &RunId, calc_id: &CalcId, status: CalcStatus) {
        if let Some(&ri) = self.run_index.get(run_id)
            && let Some(calc) =
                self.runs[ri].calculations.iter_mut().find(|c| &c.id == calc_id)
        {
            calc.status = status;
        }
    }

    /// Returns the (inclusive) range of run-list rows currently selected in visual mode.
    /// Returns `None` when visual mode is off.
    pub fn visual_range(&self) -> Option<std::ops::RangeInclusive<usize>> {
        let anchor = self.visual_anchor?;
        let lo = anchor.min(self.run_cursor);
        let hi = anchor.max(self.run_cursor);
        Some(lo..=hi)
    }

    pub fn clamp_cursors(&mut self) {
        let run_len = self.visible_runs().len();
        if run_len == 0 {
            self.run_cursor = 0;
        } else if self.run_cursor >= run_len {
            self.run_cursor = run_len - 1;
        }

        if let Some(run) = self.selected_run() {
            let calc_len = run.calculations.len();
            if calc_len == 0 {
                self.calc_cursor = 0;
            } else if self.calc_cursor >= calc_len {
                self.calc_cursor = calc_len - 1;
            }
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
