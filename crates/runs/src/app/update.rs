use common::{
    event::ServerEvent,
    model::{CalcStatus, NewCalc},
    types::CalcId,
};

use crate::{
    app::{
        keybindings::{Action, dispatch},
        msg::AppMsg,
        state::{App, ConfirmAction, ConfirmDialog, FILTER_STATUSES, Overlay, Pane, Screen},
    },
    network::NetworkCmd,
};

/// Effect requested by the update function that the main loop will execute.
#[derive(Debug)]
pub enum Effect {
    Network(NetworkCmd),
    Quit,
    SaveResult {
        calc_id: CalcId,
    },
    /// Copy `text` to the system clipboard.
    Yank(String),
}

/// Pure update: given current state and a message, return new state + effects.
pub fn update(mut app: App, msg: AppMsg) -> (App, Vec<Effect>) {
    match msg {
        AppMsg::Quit => return (app, vec![Effect::Quit]),

        AppMsg::Resize(_, _) => {}

        AppMsg::SseDisconnected => {
            app.sse_connected = false;
            app.status_bar = "SSE disconnected — reconnecting…".to_string();
        }
        AppMsg::SseReconnected => {
            app.sse_connected = true;
            app.status_bar = "Connected".to_string();
        }

        AppMsg::Tick => {
            app.tick = app.tick.wrapping_add(1);
        }

        AppMsg::RunsLoaded(runs, next_cursor) => {
            app.runs.clear();
            app.run_index.clear();
            for run in runs {
                app.upsert_run(run);
            }
            app.next_cursor = next_cursor;
            app.loading = false;
            app.status_bar = "Ready".to_string();
        }

        AppMsg::MoreRunsLoaded(runs, next_cursor) => {
            let added = runs.len();
            for run in runs {
                app.upsert_run(run);
            }
            app.next_cursor = next_cursor;
            app.status_bar = format!("Loaded {added} more runs");
        }

        AppMsg::CmdOk(msg) => {
            app.status_bar = msg;
        }
        AppMsg::CmdErr(msg) => {
            app.status_bar = format!("Error: {msg}");
        }
        AppMsg::ImportProgress {
            done,
            total,
            errors,
        } => {
            app.status_bar = if errors > 0 {
                format!("Importing… {done}/{total} ({errors} errors)")
            } else {
                format!("Importing… {done}/{total}")
            };
        }

        AppMsg::ServerEvent(sequenced) => {
            app.last_seen_seq = sequenced.seq;
            let effects = apply_server_event(&mut app, &sequenced.event);
            if !effects.is_empty() {
                return (app, effects);
            }
        }

        AppMsg::RunFetched(run) => {
            let jira = run.jira_issue_id.clone();
            app.upsert_run(run);
            app.clamp_cursors();
            app.status_bar = format!("New run: {jira}");
        }

        AppMsg::Key(key) => {
            let effects = handle_key(&mut app, key);
            return (app, effects);
        }
    }
    (app, vec![])
}

fn apply_server_event(app: &mut App, event: &ServerEvent) -> Vec<Effect> {
    match event {
        ServerEvent::CalcStatusChanged {
            run_id,
            calculation_id,
            to,
            ..
        } => {
            app.update_calc_status(run_id, calculation_id, *to);
        }
        ServerEvent::RunSubmitted { run_id, .. } => {
            // Fetch the full run so it appears in the list immediately.
            return vec![Effect::Network(NetworkCmd::FetchRun {
                run_id: run_id.clone(),
            })];
        }
        _ => {}
    }
    vec![]
}

fn handle_key(app: &mut App, key: crossterm::event::KeyEvent) -> Vec<Effect> {
    // Overlay / command mode takes priority.
    if let Overlay::Command(buf) = app.overlay.clone() {
        return handle_command_input(app, key, buf);
    }
    if app.overlay == Overlay::Help {
        match key.code {
            crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Char('q') => {
                app.overlay = Overlay::None;
            }
            _ => {}
        }
        return vec![];
    }

    if app.overlay == Overlay::Filter {
        match key.code {
            crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Char('q') => {
                app.overlay = Overlay::None;
            }
            crossterm::event::KeyCode::Char('j') | crossterm::event::KeyCode::Down => {
                let max = FILTER_STATUSES.len() - 1;
                if app.filter.filter_cursor < max {
                    app.filter.filter_cursor += 1;
                }
            }
            crossterm::event::KeyCode::Char('k') | crossterm::event::KeyCode::Up => {
                app.filter.filter_cursor = app.filter.filter_cursor.saturating_sub(1);
            }
            crossterm::event::KeyCode::Enter => {
                let chosen = FILTER_STATUSES[app.filter.filter_cursor];
                app.filter.status = if chosen == "all" {
                    None
                } else {
                    Some(chosen.to_string())
                };
                app.run_cursor = 0;
                app.overlay = Overlay::None;
            }
            _ => {}
        }
        return vec![];
    }
    if let Overlay::Confirm(ref dialog) = app.overlay.clone() {
        return handle_confirm(app, key, dialog.clone());
    }

    // Vim count prefix: accumulate digits before a motion.
    if app.overlay == Overlay::None
        && let crossterm::event::KeyCode::Char(c) = key.code
        && c.is_ascii_digit()
        && key.modifiers == crossterm::event::KeyModifiers::NONE
        // Don't start with 0 unless already accumulating.
        && (c != '0' || !app.count_buf.is_empty())
    {
        app.count_buf.push(c);
        return vec![];
    }

    let count = app.count_buf.parse::<usize>().unwrap_or(1).max(1);
    app.count_buf.clear();

    let action = dispatch(&key);
    match action {
        Action::Quit => {
            if matches!(app.screen, Screen::RunList) {
                return vec![Effect::Quit];
            } else {
                app.screen = Screen::RunList;
                app.pane = Pane::RunList;
            }
        }
        Action::Back => {
            // Esc exits visual mode first, then navigates back.
            if app.visual_anchor.is_some() {
                app.visual_anchor = None;
                return vec![];
            }
            app.overlay = Overlay::None;
            match &app.screen {
                Screen::CalcDetail(_) => {
                    app.screen = Screen::RunList;
                    app.pane = Pane::CalcList;
                }
                Screen::RunDetail(_) | Screen::Dashboard => {
                    app.screen = Screen::RunList;
                    app.pane = Pane::RunList;
                }
                Screen::RunList => {
                    app.pane = Pane::RunList;
                }
            }
        }
        Action::Help => {
            app.overlay = Overlay::Help;
        }
        Action::Dashboard => {
            app.screen = if app.screen == Screen::Dashboard {
                Screen::RunList
            } else {
                Screen::Dashboard
            };
        }
        Action::FilterOverlay => {
            // Initialise overlay cursor to the currently active filter.
            let current = app.filter.status.as_deref().unwrap_or("all");
            app.filter.filter_cursor = FILTER_STATUSES
                .iter()
                .position(|&s| s == current)
                .unwrap_or(0);
            app.overlay = Overlay::Filter;
        }
        Action::CommandMode => {
            app.overlay = Overlay::Command(String::new());
        }
        Action::Search => {
            app.overlay = Overlay::Command("/".to_string());
        }
        Action::FocusPrev => {
            app.pane = match app.pane {
                Pane::RunList => Pane::RunList,
                Pane::CalcList => Pane::RunList,
                Pane::Detail => Pane::CalcList,
            };
        }
        Action::FocusNext => {
            app.pane = match app.pane {
                Pane::RunList => Pane::CalcList,
                Pane::CalcList => Pane::Detail,
                Pane::Detail => Pane::Detail,
            };
        }
        Action::MoveDown => {
            if app.pane == Pane::Detail {
                app.detail_scroll += count * 3;
            } else {
                for _ in 0..count {
                    move_cursor_down(app);
                }
                // Trigger load-more when reaching the bottom of the run list.
                if app.pane == Pane::RunList
                    && app.run_cursor + 1 >= app.visible_runs().len()
                    && let Some(cursor) = app.next_cursor.take()
                {
                    return vec![Effect::Network(NetworkCmd::LoadMoreRuns { cursor })];
                }
            }
        }
        Action::MoveUp => {
            if app.pane == Pane::Detail {
                app.detail_scroll = app.detail_scroll.saturating_sub(count * 3);
            } else {
                for _ in 0..count {
                    move_cursor_up(app);
                }
            }
        }
        Action::GotoTop => {
            if app.pane == Pane::Detail {
                app.detail_scroll = 0;
            } else if app.pane == Pane::RunList {
                app.run_cursor = 0;
            } else {
                app.calc_cursor = 0;
            }
        }
        Action::GotoBottom => {
            if app.pane == Pane::Detail {
                app.detail_scroll = usize::MAX / 2; // clamp in render
            } else if app.pane == Pane::RunList {
                let len = app.visible_runs().len();
                app.run_cursor = len.saturating_sub(1);
            } else if let Some(run) = app.selected_run() {
                let len = run.calculations.len();
                app.calc_cursor = len.saturating_sub(1);
            }
        }
        Action::HalfPageDown => {
            let step = 10usize * count;
            if app.pane == Pane::Detail {
                app.detail_scroll += step;
            } else if app.pane == Pane::RunList {
                let max = app.visible_runs().len().saturating_sub(1);
                app.run_cursor = (app.run_cursor + step).min(max);
            } else if let Some(run) = app.selected_run() {
                let max = run.calculations.len().saturating_sub(1);
                app.calc_cursor = (app.calc_cursor + step).min(max);
            }
        }
        Action::HalfPageUp => {
            let step = 10usize * count;
            if app.pane == Pane::Detail {
                app.detail_scroll = app.detail_scroll.saturating_sub(step);
            } else if app.pane == Pane::RunList {
                app.run_cursor = app.run_cursor.saturating_sub(step);
            } else {
                app.calc_cursor = app.calc_cursor.saturating_sub(step);
            }
        }
        Action::Enter => match app.pane {
            Pane::RunList => {
                if let Some(run) = app.selected_run() {
                    let rid = run.id.clone();
                    app.screen = Screen::RunDetail(rid);
                    app.pane = Pane::CalcList;
                    app.calc_cursor = 0;
                    app.detail_scroll = 0;
                }
            }
            Pane::CalcList => {
                if let Some(calc) = app.selected_calc() {
                    let cid = calc.id.clone();
                    app.screen = Screen::CalcDetail(cid);
                    app.pane = Pane::Detail;
                    app.detail_scroll = 0;
                }
            }
            Pane::Detail => {}
        },
        Action::Save => {
            if let Some(calc) = app.selected_calc() {
                if calc.result_path.is_some() {
                    let cid = calc.id.clone();
                    return vec![Effect::SaveResult { calc_id: cid }];
                } else {
                    app.status_bar = "No result available".into();
                }
            }
        }
        Action::Yank => {
            let text = match app.pane {
                Pane::CalcList | Pane::Detail => app.selected_calc().map(|c| c.id.to_string()),
                Pane::RunList => app.selected_run().map(|r| r.id.to_string()),
            };
            if let Some(t) = text {
                app.status_bar = format!("Yanked: {t}");
                return vec![Effect::Yank(t)];
            }
        }

        Action::VisualToggle => {
            if app.pane == Pane::RunList {
                app.visual_anchor = if app.visual_anchor.is_some() {
                    None
                } else {
                    Some(app.run_cursor)
                };
            }
        }

        Action::Refresh => {
            app.loading = true;
            app.status_bar = "Refreshing…".to_string();
            app.visual_anchor = None;
            return vec![Effect::Network(NetworkCmd::RefreshRuns)];
        }

        // Bulk cancel: in visual mode cancel all selected runs; otherwise single.
        Action::Cancel => {
            if let Some(range) = app.visual_range() {
                let ids: Vec<_> = app
                    .visible_runs()
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| range.contains(i))
                    .map(|(_, r)| r.id.clone())
                    .collect();
                let n = ids.len();
                app.visual_anchor = None;
                app.status_bar = format!("Cancelling {n} runs…");
                return ids
                    .into_iter()
                    .map(|run_id| Effect::Network(NetworkCmd::CancelRun { run_id }))
                    .collect();
            }
            match app.pane {
                Pane::RunList => {
                    if let Some(run) = app.selected_run() {
                        let rid = run.id.clone();
                        app.overlay = Overlay::Confirm(ConfirmDialog {
                            message: format!("Cancel run {}? (y/N)", run.jira_issue_id),
                            action: ConfirmAction::CancelRun(rid),
                        });
                    }
                }
                Pane::CalcList | Pane::Detail => {
                    if let Some(calc) = app.selected_calc() {
                        let cid = calc.id.clone();
                        app.overlay = Overlay::Confirm(ConfirmDialog {
                            message: format!("Cancel calculation {}? (y/N)", calc.kind),
                            action: ConfirmAction::CancelCalc(cid),
                        });
                    }
                }
            }
        }

        // Bulk retry: in visual mode retry all failed calcs across selected runs.
        Action::Retry => {
            if let Some(range) = app.visual_range() {
                let effects: Vec<_> = app
                    .visible_runs()
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| range.contains(i))
                    .flat_map(|(_, run)| {
                        run.calculations
                            .iter()
                            .filter(|c| c.status == CalcStatus::Failed)
                            .map(|c| {
                                Effect::Network(NetworkCmd::RetryCalc {
                                    run_id: run.id.clone(),
                                    calc_id: c.id.clone(),
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect();
                app.visual_anchor = None;
                if effects.is_empty() {
                    app.status_bar = "No failed calculations in selection".into();
                    return vec![];
                }
                app.status_bar = format!("Retrying {} calculations…", effects.len());
                return effects;
            }
            if let Some(calc) = app.selected_calc() {
                if calc.status == CalcStatus::Failed {
                    let cid = calc.id.clone();
                    let rid = calc.run_id.clone();
                    return vec![Effect::Network(NetworkCmd::RetryCalc {
                        run_id: rid,
                        calc_id: cid,
                    })];
                } else {
                    app.status_bar = "Only failed calculations can be retried".into();
                }
            }
        }

        Action::SearchNext => advance_search(app, 1),
        Action::SearchPrev => advance_search(app, -1),
        Action::Unknown => {}
    }
    vec![]
}

fn handle_confirm(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    dialog: ConfirmDialog,
) -> Vec<Effect> {
    match key.code {
        crossterm::event::KeyCode::Char('y') | crossterm::event::KeyCode::Char('Y') => {
            app.overlay = Overlay::None;
            match dialog.action {
                ConfirmAction::Quit => return vec![Effect::Quit],
                ConfirmAction::CancelRun(rid) => {
                    return vec![Effect::Network(NetworkCmd::CancelRun { run_id: rid })];
                }
                ConfirmAction::CancelCalc(cid) => {
                    if let Some(calc) = app
                        .runs
                        .iter()
                        .flat_map(|r| r.calculations.iter())
                        .find(|c| c.id == cid)
                    {
                        let rid = calc.run_id.clone();
                        return vec![Effect::Network(NetworkCmd::CancelCalc {
                            run_id: rid,
                            calc_id: cid,
                        })];
                    }
                }
                ConfirmAction::RetryCalc(cid) => {
                    if let Some(calc) = app
                        .runs
                        .iter()
                        .flat_map(|r| r.calculations.iter())
                        .find(|c| c.id == cid)
                    {
                        let rid = calc.run_id.clone();
                        return vec![Effect::Network(NetworkCmd::RetryCalc {
                            run_id: rid,
                            calc_id: cid,
                        })];
                    }
                }
            }
        }
        _ => {
            app.overlay = Overlay::None;
        }
    }
    vec![]
}

fn handle_command_input(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    mut buf: String,
) -> Vec<Effect> {
    match key.code {
        crossterm::event::KeyCode::Esc => {
            app.overlay = Overlay::None;
        }
        crossterm::event::KeyCode::Enter => {
            app.overlay = Overlay::None;
            return execute_command(app, &buf);
        }
        crossterm::event::KeyCode::Backspace => {
            buf.pop();
            app.overlay = Overlay::Command(buf);
        }
        crossterm::event::KeyCode::Char(c) => {
            buf.push(c);
            app.overlay = Overlay::Command(buf);
        }
        _ => {}
    }
    vec![]
}

fn execute_command(app: &mut App, cmd: &str) -> Vec<Effect> {
    let cmd = cmd.trim();

    if let Some(query) = cmd.strip_prefix('/') {
        // Search command.
        app.filter.search = if query.is_empty() {
            None
        } else {
            Some(query.to_string())
        };
        app.filter.search_pos = 0;
        return vec![];
    }

    if let Some(rest) = cmd.strip_prefix("filter ") {
        // :filter status=failed
        if let Some(status) = rest.strip_prefix("status=") {
            app.filter.status = if status == "all" || status.is_empty() {
                None
            } else {
                Some(status.to_string())
            };
        }
        return vec![];
    }

    if cmd == "quit" || cmd == "q" {
        return vec![Effect::Quit];
    }

    if cmd == "reload" || cmd == "refresh" {
        app.loading = true;
        app.status_bar = "Refreshing…".to_string();
        return vec![Effect::Network(NetworkCmd::RefreshRuns)];
    }

    if let Some(raw_path) = cmd.strip_prefix("import ") {
        let path = std::path::Path::new(raw_path.trim()).to_path_buf();
        return vec![Effect::Network(NetworkCmd::ImportDirectory { path })];
    }

    // :submit <jira_id> <kind1> [kind2 ...]
    if let Some(rest) = cmd.strip_prefix("submit ") {
        let mut parts = rest.split_whitespace();
        if let Some(jira_id) = parts.next() {
            let kinds: Vec<&str> = parts.collect();
            if kinds.is_empty() {
                app.status_bar = "Usage: submit <jira_id> <kind1> [kind2...]".into();
            } else {
                let calcs = kinds
                    .into_iter()
                    .map(|k| NewCalc {
                        kind: k.to_string(),
                        input: serde_json::Value::Object(Default::default()),
                    })
                    .collect();
                return vec![Effect::Network(NetworkCmd::SubmitRun {
                    jira_issue_id: jira_id.to_string(),
                    calcs,
                })];
            }
        } else {
            app.status_bar = "Usage: submit <jira_id> <kind1> [kind2...]".into();
        }
        return vec![];
    }

    app.status_bar = format!("Unknown command: {cmd}");
    vec![]
}

fn move_cursor_down(app: &mut App) {
    if app.pane == Pane::RunList {
        let max = app.visible_runs().len().saturating_sub(1);
        if app.run_cursor < max {
            app.run_cursor += 1;
        }
        app.calc_cursor = 0;
    } else {
        if let Some(run) = app.selected_run() {
            let max = run.calculations.len().saturating_sub(1);
            if app.calc_cursor < max {
                app.calc_cursor += 1;
            }
        }
    }
}

fn move_cursor_up(app: &mut App) {
    if app.pane == Pane::RunList {
        app.run_cursor = app.run_cursor.saturating_sub(1);
        app.calc_cursor = 0;
    } else {
        app.calc_cursor = app.calc_cursor.saturating_sub(1);
    }
}

fn advance_search(app: &mut App, delta: i32) {
    let total = app.filter.search_matches.len();
    if total == 0 {
        return;
    }
    let pos = app.filter.search_pos as i32 + delta;
    app.filter.search_pos = pos.rem_euclid(total as i32) as usize;
    app.run_cursor = app.filter.search_matches[app.filter.search_pos];
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use common::{
        model::{CalcStatus, Calculation, Run, RunStatus},
        types::{CalcId, RunId},
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use uuid::Uuid;

    // ── helpers ───────────────────────────────────────────────────────────────

    fn make_run(jira: &str) -> Run {
        Run {
            id: RunId(Uuid::now_v7()),
            jira_issue_id: jira.to_string(),
            submitted_by: "test".into(),
            status: RunStatus::Running,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            calculations: vec![],
        }
    }

    fn make_calc(run_id: RunId, status: CalcStatus) -> Calculation {
        Calculation {
            id: CalcId(Uuid::now_v7()),
            run_id,
            kind: "test_calc".into(),
            input_json: serde_json::Value::Null,
            idempotency_key: "k".into(),
            status,
            attempt: 1,
            max_attempts: 3,
            next_attempt_at: None,
            lease_owner: None,
            lease_expires_at: None,
            error_kind: None,
            error_message: None,
            result_path: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            updated_at: Utc::now(),
        }
    }

    fn key(code: KeyCode) -> AppMsg {
        AppMsg::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn ctrl(code: KeyCode) -> AppMsg {
        AppMsg::Key(KeyEvent::new(code, KeyModifiers::CONTROL))
    }

    fn app_with_runs(n: usize) -> App {
        let mut app = App::new();
        app.loading = false;
        for i in 0..n {
            app.upsert_run(make_run(&format!("JIRA-{i}")));
        }
        app
    }

    // ── cursor movement ───────────────────────────────────────────────────────

    #[test]
    fn j_moves_cursor_down() {
        let mut app = app_with_runs(3);
        assert_eq!(app.run_cursor, 0);
        (app, _) = update(app, key(KeyCode::Char('j')));
        assert_eq!(app.run_cursor, 1);
        (app, _) = update(app, key(KeyCode::Char('j')));
        assert_eq!(app.run_cursor, 2);
    }

    #[test]
    fn j_does_not_go_past_end() {
        let mut app = app_with_runs(2);
        (app, _) = update(app, key(KeyCode::Char('j')));
        (app, _) = update(app, key(KeyCode::Char('j')));
        (app, _) = update(app, key(KeyCode::Char('j')));
        assert_eq!(app.run_cursor, 1);
    }

    #[test]
    fn k_moves_cursor_up() {
        let mut app = app_with_runs(3);
        (app, _) = update(app, key(KeyCode::Char('j')));
        (app, _) = update(app, key(KeyCode::Char('j')));
        assert_eq!(app.run_cursor, 2);
        (app, _) = update(app, key(KeyCode::Char('k')));
        assert_eq!(app.run_cursor, 1);
    }

    #[test]
    fn k_does_not_go_below_zero() {
        let mut app = app_with_runs(2);
        (app, _) = update(app, key(KeyCode::Char('k')));
        (app, _) = update(app, key(KeyCode::Char('k')));
        assert_eq!(app.run_cursor, 0);
    }

    #[test]
    fn g_goes_to_top_g_goes_to_bottom() {
        let mut app = app_with_runs(5);
        (app, _) = update(app, key(KeyCode::Char('G')));
        assert_eq!(app.run_cursor, 4);
        (app, _) = update(app, key(KeyCode::Char('g')));
        assert_eq!(app.run_cursor, 0);
    }

    #[test]
    fn count_prefix_5j_moves_5_rows() {
        let mut app = app_with_runs(10);
        for c in "5".chars() {
            (app, _) = update(app, key(KeyCode::Char(c)));
        }
        (app, _) = update(app, key(KeyCode::Char('j')));
        assert_eq!(app.run_cursor, 5);
    }

    // ── RunsLoaded ────────────────────────────────────────────────────────────

    #[test]
    fn runs_loaded_sets_loading_false() {
        let app = App::new();
        assert!(app.loading);
        let (app, _) = update(app, AppMsg::RunsLoaded(vec![], None));
        assert!(!app.loading);
    }

    #[test]
    fn runs_loaded_populates_list() {
        let app = App::new();
        let runs = vec![make_run("JIRA-1"), make_run("JIRA-2")];
        let (app, _) = update(app, AppMsg::RunsLoaded(runs, None));
        assert_eq!(app.runs.len(), 2);
        assert_eq!(app.next_cursor, None);
    }

    #[test]
    fn more_runs_loaded_appends() {
        let app = App::new();
        let (app, _) = update(app, AppMsg::RunsLoaded(vec![make_run("A")], None));
        let (app, _) = update(app, AppMsg::MoreRunsLoaded(vec![make_run("B")], None));
        assert_eq!(app.runs.len(), 2);
    }

    // ── Tick ──────────────────────────────────────────────────────────────────

    #[test]
    fn tick_increments_counter() {
        let app = App::new();
        let (app, _) = update(app, AppMsg::Tick);
        assert_eq!(app.tick, 1);
        let (app, _) = update(app, AppMsg::Tick);
        assert_eq!(app.tick, 2);
    }

    // ── Command mode: :submit ─────────────────────────────────────────────────

    #[test]
    fn submit_command_returns_submit_run_effect() {
        let app = App::new();
        // Open command mode
        let (mut app, _) = update(app, key(KeyCode::Char(':')));
        assert!(matches!(app.overlay, Overlay::Command(_)));
        // Type "submit JIRA-42 calc_a calc_b"
        for c in "submit JIRA-42 calc_a calc_b".chars() {
            (app, _) = update(app, key(KeyCode::Char(c)));
        }
        let (_, effects) = update(app, key(KeyCode::Enter));
        let has_submit = effects.iter().any(|e| {
            matches!(e, Effect::Network(NetworkCmd::SubmitRun { jira_issue_id, calcs })
                if jira_issue_id == "JIRA-42" && calcs.len() == 2)
        });
        assert!(has_submit, "expected SubmitRun effect");
    }

    #[test]
    fn submit_command_with_no_kinds_sets_status_bar() {
        let app = App::new();
        let (mut app, _) = update(app, key(KeyCode::Char(':')));
        for c in "submit JIRA-1".chars() {
            (app, _) = update(app, key(KeyCode::Char(c)));
        }
        let (app, effects) = update(app, key(KeyCode::Enter));
        assert!(effects.is_empty());
        assert!(
            app.status_bar.contains("Usage"),
            "expected usage message, got: {}",
            app.status_bar
        );
    }

    // ── Visual mode ───────────────────────────────────────────────────────────

    #[test]
    fn v_enters_visual_mode() {
        let app = app_with_runs(3);
        let (app, _) = update(app, key(KeyCode::Char('v')));
        assert_eq!(app.visual_anchor, Some(0));
    }

    #[test]
    fn v_twice_exits_visual_mode() {
        let app = app_with_runs(3);
        let (app, _) = update(app, key(KeyCode::Char('v')));
        let (app, _) = update(app, key(KeyCode::Char('v')));
        assert_eq!(app.visual_anchor, None);
    }

    #[test]
    fn esc_exits_visual_mode_before_navigating_back() {
        let app = app_with_runs(3);
        let (app, _) = update(app, key(KeyCode::Char('v')));
        assert!(app.visual_anchor.is_some());
        let (app, _) = update(app, key(KeyCode::Esc));
        assert!(app.visual_anchor.is_none());
        // Screen should still be RunList (didn't navigate back)
        assert_eq!(app.screen, crate::app::state::Screen::RunList);
    }

    #[test]
    fn visual_bulk_cancel_returns_one_effect_per_selected_run() {
        let mut app = app_with_runs(4);
        // Enter visual mode at row 0, extend to row 2
        (app, _) = update(app, key(KeyCode::Char('v')));
        (app, _) = update(app, key(KeyCode::Char('j')));
        (app, _) = update(app, key(KeyCode::Char('j')));
        assert_eq!(app.visual_range(), Some(0..=2));
        let (app, effects) = update(app, key(KeyCode::Char('X')));
        let cancel_count = effects
            .iter()
            .filter(|e| matches!(e, Effect::Network(NetworkCmd::CancelRun { .. })))
            .count();
        assert_eq!(cancel_count, 3);
        assert!(
            app.visual_anchor.is_none(),
            "visual mode should be cleared after bulk op"
        );
    }

    // ── Bulk retry ────────────────────────────────────────────────────────────

    #[test]
    fn visual_bulk_retry_only_retries_failed_calcs() {
        let mut app = App::new();
        let mut run_a = make_run("A");
        let mut run_b = make_run("B");
        let calc_failed = make_calc(run_a.id.clone(), CalcStatus::Failed);
        let calc_ok = make_calc(run_b.id.clone(), CalcStatus::Succeeded);
        run_a.calculations.push(calc_failed);
        run_b.calculations.push(calc_ok);
        app.upsert_run(run_a);
        app.upsert_run(run_b);

        (app, _) = update(app, key(KeyCode::Char('v')));
        (app, _) = update(app, key(KeyCode::Char('j'))); // extend to row 1
        let (_, effects) = update(app, key(KeyCode::Char('R')));
        let retry_count = effects
            .iter()
            .filter(|e| matches!(e, Effect::Network(NetworkCmd::RetryCalc { .. })))
            .count();
        assert_eq!(retry_count, 1, "only the failed calc should be retried");
    }

    // ── Ctrl-c / q ───────────────────────────────────────────────────────────

    #[test]
    fn ctrl_c_on_run_list_quits_immediately() {
        let app = App::new();
        let (_, effects) = update(app, ctrl(KeyCode::Char('c')));
        assert!(effects.iter().any(|e| matches!(e, Effect::Quit)));
    }

    #[test]
    fn q_on_run_list_quits_immediately() {
        let app = App::new();
        let (_, effects) = update(app, key(KeyCode::Char('q')));
        assert!(effects.iter().any(|e| matches!(e, Effect::Quit)));
    }

    #[test]
    fn q_on_run_detail_goes_back_to_run_list() {
        let mut app = app_with_runs(1);
        (app, _) = update(app, key(KeyCode::Enter));
        assert!(matches!(app.screen, Screen::RunDetail(_)));
        let (app, effects) = update(app, key(KeyCode::Char('q')));
        assert!(effects.iter().all(|e| !matches!(e, Effect::Quit)));
        assert_eq!(app.screen, Screen::RunList);
    }

    // ── Refresh ───────────────────────────────────────────────────────────────

    #[test]
    fn r_key_returns_refresh_effect() {
        let app = App::new();
        let (app, effects) = update(app, key(KeyCode::Char('r')));
        assert!(app.loading);
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::Network(NetworkCmd::RefreshRuns)))
        );
    }
}
