use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Every action the TUI can perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    // Navigation
    FocusPrev,
    FocusNext,
    MoveDown,
    MoveUp,
    GotoTop,
    GotoBottom,
    HalfPageDown,
    HalfPageUp,
    Enter,
    Back,

    // Verbs (uppercase)
    Retry,
    Save,
    Cancel,
    Yank,

    // Visual mode
    VisualToggle,

    // Misc
    Refresh,

    // Overlays / modes
    Search,
    SearchNext,
    SearchPrev,
    CommandMode,
    FilterOverlay,
    Help,
    Dashboard,

    Quit,
    Unknown,
}

pub fn dispatch(key: &KeyEvent) -> Action {
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Char('h')) => Action::FocusPrev,
        (KeyModifiers::NONE, KeyCode::Char('l')) => Action::FocusNext,
        (KeyModifiers::NONE, KeyCode::Char('j')) => Action::MoveDown,
        (KeyModifiers::NONE, KeyCode::Char('k')) => Action::MoveUp,
        (KeyModifiers::NONE, KeyCode::Char('g')) => Action::GotoTop,
        (KeyModifiers::NONE, KeyCode::Char('G')) => Action::GotoBottom,
        (KeyModifiers::CONTROL, KeyCode::Char('d')) => Action::HalfPageDown,
        (KeyModifiers::CONTROL, KeyCode::Char('u')) => Action::HalfPageUp,
        (KeyModifiers::NONE, KeyCode::Enter) => Action::Enter,
        (KeyModifiers::NONE, KeyCode::Esc) => Action::Back,

        // Verb keys (uppercase).
        (KeyModifiers::SHIFT, KeyCode::Char('R')) | (KeyModifiers::NONE, KeyCode::Char('R')) => {
            Action::Retry
        }
        (KeyModifiers::SHIFT, KeyCode::Char('S')) | (KeyModifiers::NONE, KeyCode::Char('S')) => {
            Action::Save
        }
        (KeyModifiers::SHIFT, KeyCode::Char('X')) | (KeyModifiers::NONE, KeyCode::Char('X')) => {
            Action::Cancel
        }

        (KeyModifiers::NONE, KeyCode::Char('y')) => Action::Yank,
        (KeyModifiers::NONE, KeyCode::Char('v')) => Action::VisualToggle,
        (KeyModifiers::CONTROL, KeyCode::Char('r')) => Action::Refresh,
        (KeyModifiers::NONE, KeyCode::Char('r')) => Action::Refresh,
        (KeyModifiers::NONE, KeyCode::Char('/')) => Action::Search,
        (KeyModifiers::NONE, KeyCode::Char('n')) => Action::SearchNext,
        (KeyModifiers::SHIFT, KeyCode::Char('N')) | (KeyModifiers::NONE, KeyCode::Char('N')) => {
            Action::SearchPrev
        }
        (KeyModifiers::NONE, KeyCode::Char(':')) => Action::CommandMode,
        (KeyModifiers::NONE, KeyCode::Char('f')) => Action::FilterOverlay,
        (KeyModifiers::NONE, KeyCode::Char('?')) => Action::Help,
        (KeyModifiers::SHIFT, KeyCode::Char('D')) | (KeyModifiers::NONE, KeyCode::Char('D')) => Action::Dashboard,
        (KeyModifiers::NONE, KeyCode::Char('q')) => Action::Quit,
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => Action::Quit,
        _ => Action::Unknown,
    }
}

/// A group of related keybindings shown together in the help overlay.
pub struct HelpSection {
    pub title: &'static str,
    pub entries: &'static [(&'static str, &'static str)],
}

/// Keybinding table grouped by section, matching dispatch() exactly.
pub fn help_sections() -> &'static [HelpSection] {
    &[
        HelpSection {
            title: "Navigation",
            entries: &[
                ("h / l",        "Focus prev / next pane"),
                ("j / k",        "Move down / up"),
                ("g / G",        "Top / bottom of list"),
                ("Ctrl-d / u",   "Half-page down / up"),
                ("Enter",        "Drill into selection"),
                ("Esc",          "Back / close overlay"),
            ],
        },
        HelpSection {
            title: "Actions",
            entries: &[
                ("R",  "Retry selected calculation"),
                ("S",  "Save result to disk"),
                ("X",  "Cancel selected run or calculation"),
                ("y",  "Yank selected ID to clipboard"),
                ("v",  "Visual mode (multi-select runs)"),
                ("r",  "Refresh run list"),
            ],
        },
        HelpSection {
            title: "Search & Filter",
            entries: &[
                ("/",    "Enter search"),
                ("n / N","Next / prev match"),
                ("f",    "Filter overlay"),
            ],
        },
        HelpSection {
            title: "General",
            entries: &[
                (":",                          "Command mode"),
                (":import <path>",             "Import JSON runs from directory"),
                (":submit <jira> <kind...>",   "Submit a new run"),
                ("<count>j/k",                 "Move N rows (e.g. 5j)"),
                ("D",                          "Dashboard / history overview"),
                ("?",                          "This help screen"),
                ("q",                          "Quit"),
            ],
        },
    ]
}

/// Flat list kept for any code that still wants a simple iterator.
pub fn help_entries() -> Vec<(&'static str, &'static str)> {
    help_sections()
        .iter()
        .flat_map(|s| s.entries.iter().copied())
        .collect()
}
