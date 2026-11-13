use aether_ui::{
    AGENT_SELECTOR_CONTEXT, BRANCH_SELECT_CONTEXT, CHOICE_MENU_CONTEXT, ChoiceConfirm,
    ChoiceDismiss, ChoiceDown, ChoiceUp, SelectConfirm, SelectDismiss, SelectNext, SelectPrev,
    SelectorConfirm, SelectorDismiss, SelectorNext, SelectorNextTab, SelectorPrev, SelectorPrevTab,
};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::{Action, App, AsKeystroke, KeyBinding, actions};

pub use aether_terminal::ToggleTerminal;

use crate::explorer::quick_open::{
    QUICK_OPEN_CONTEXT, QuickOpenConfirm, QuickOpenDismiss, QuickOpenNext, QuickOpenPrev,
};
use crate::menus::{CloseWindow, Hide, HideOthers, Minimize, Quit};
use crate::workspace_popover::{
    BRANCH_SEARCH_CONTEXT, POP_CONTEXT, PUBLISH_NAME_CONTEXT, PopBack, PopConfirm, PopDown, PopUp,
    SCRIPT_FORM_CONTEXT,
};

actions!(
    aether,
    [
        FocusSessionLeft,
        FocusSessionRight,
        FocusSessionUp,
        FocusSessionDown,
        NextSessionPane,
        PreviousSessionPane,
        CycleTileLayout,
        CycleChatLayout,
        CycleOrgLayout,
        SwapSessionLeft,
        SwapSessionRight,
        SwapSessionUp,
        SwapSessionDown,
        PromoteSession,
        NarrowMainPane,
        WidenMainPane,
        ToggleSessionFloat,
        ToggleCommandMenu,
        ToggleGlobalTerminal,
        FocusComposer,
        PreviousSideTab,
        NextSideTab,
        NewChat,
        OpenSettings,
        SettingsNext,
        SettingsPrevious,
        SettingsDismiss,
        SettingsSearch,
        OpenSkills,
        OpenNotes,
        SearchNotes,
        NotesSearchNext,
        NotesSearchPrev,
        NotesSearchConfirm,
        NotesSearchDismiss,
        SaveSelectionToNote,
        OpenUsage,
        OpenBoard,
        OpenOrg,
        ToggleHackerNews,
        BackToChat,
        SearchWorkspace,
        ToggleWebSearch,
        StopGenerating,
        SuggestionPrev,
        SuggestionNext,
        SuggestionAccept,
        SuggestionDismiss,
        ComposerHistoryPrev,
        ComposerHistoryNext,
        ToggleChanges,
        ToggleBrowser,
        ToggleWatchList,
        ToggleSidePanel,
        ToggleSidebar,
        CommitChanges,
        ToggleExplorer,
        QuickOpen,
        SaveBuffer,
        EditDraft,
        ToggleVoice,
        DraftEditorDone,
        DraftEditorSend,
        AskPrimary,
        AskDismiss,
        AskPrev,
        AskNext,
        AskToggle
    ]
);

/// Key context set around the composer while a suggestion menu is open.
pub const SUGGESTION_CONTEXT: &str = "SuggestionMenu";
/// Key context set around the composer when the chat has message history.
pub const COMPOSER_HISTORY_CONTEXT: &str = "ComposerHistory";
/// Key context around the changes panel's commit message field.
pub const COMMIT_CONTEXT: &str = "GitCommit";
/// Key context always set around the composer.
pub const COMPOSER_CONTEXT: &str = "Composer";
/// Key context around the Ctrl+G draft editor sheet.
pub const DRAFT_EDITOR_CONTEXT: &str = "DraftEditor";
/// Key context set around the composer while the active chat generates.
pub const GENERATING_CONTEXT: &str = "Generating";
/// Key context on a pending approval, question, form or plan card.
pub const ASK_CONTEXT: &str = "AskCard";

/// Platform text for `action`'s shortcut, e.g. "⌘P" or "Ctrl+P", ignoring key context.
pub fn key_hint(action: &dyn Action, cx: &App) -> Option<String> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let key = keymap.bindings_for_action(action).last()?.keystrokes().first()?;
    Some(Kbd::format(key.as_keystroke()))
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("alt-n", OpenNotes, None),
        KeyBinding::new("alt-b", OpenBoard, None),
        KeyBinding::new("alt-o", OpenOrg, None),
        KeyBinding::new("secondary-shift-n", SearchNotes, None),
        KeyBinding::new(
            "/",
            FocusComposer,
            Some("(Chat || AetherSession) && !Input && !Editor && !Terminal && !PopupMenu && !ChoiceMenu && !BranchSelect && !AgentSelector && !WorkspaceSelect && !Select && !Command && !AskCard"),
        ),
        KeyBinding::new("secondary-b", ToggleSidebar, None),
        KeyBinding::new("secondary-k", ToggleCommandMenu, None),
        KeyBinding::new("secondary-t", ToggleGlobalTerminal, None),
        KeyBinding::new("secondary-n", NewChat, None),
        KeyBinding::new("secondary-,", OpenSettings, None),
        KeyBinding::new("secondary-u", OpenUsage, None),
        KeyBinding::new("secondary-shift-y", ToggleHackerNews, None),
        KeyBinding::new("secondary-h", ToggleHackerNews, None),
        KeyBinding::new("secondary-shift-g", ToggleChanges, None),
        KeyBinding::new("secondary-shift-b", ToggleBrowser, None),
        KeyBinding::new("secondary-shift-w", ToggleWatchList, None),
        KeyBinding::new("secondary-e", ToggleExplorer, None),
        KeyBinding::new("secondary-p", QuickOpen, None),
        KeyBinding::new("secondary-s", SaveBuffer, None),
        KeyBinding::new("secondary-alt-b", ToggleSidePanel, None),
        KeyBinding::new("secondary-shift-h", PreviousSideTab, None),
        KeyBinding::new("secondary-shift-l", NextSideTab, None),
    ]);

    // before the focus keys: on Linux secondary-alt-[ is ctrl-alt-[ and Previous pane must win
    let tiles = Some("SessionWorkspace && !Terminal && !Editor && !AskCard");
    cx.bind_keys([
        KeyBinding::new("ctrl-alt-l", CycleTileLayout, tiles),
        KeyBinding::new("ctrl-alt-shift-left", SwapSessionLeft, tiles),
        KeyBinding::new("ctrl-alt-shift-right", SwapSessionRight, tiles),
        KeyBinding::new("ctrl-alt-shift-up", SwapSessionUp, tiles),
        KeyBinding::new("ctrl-alt-shift-down", SwapSessionDown, tiles),
        KeyBinding::new("ctrl-alt-enter", PromoteSession, tiles),
        KeyBinding::new("ctrl-alt-[", NarrowMainPane, tiles),
        KeyBinding::new("ctrl-alt-]", WidenMainPane, tiles),
        KeyBinding::new("ctrl-alt-f", ToggleSessionFloat, tiles),
    ]);
    cx.bind_keys([
        KeyBinding::new(
            "secondary-alt-left",
            FocusSessionLeft,
            Some("SessionWorkspace && !Terminal && !Editor && !AskCard"),
        ),
        KeyBinding::new(
            "secondary-alt-right",
            FocusSessionRight,
            Some("SessionWorkspace && !Terminal && !Editor && !AskCard"),
        ),
        KeyBinding::new(
            "secondary-alt-up",
            FocusSessionUp,
            Some("SessionWorkspace && !Terminal && !Editor && !AskCard"),
        ),
        KeyBinding::new(
            "secondary-alt-down",
            FocusSessionDown,
            Some("SessionWorkspace && !Terminal && !Editor && !AskCard"),
        ),
        KeyBinding::new(
            "secondary-alt-]",
            NextSessionPane,
            Some("SessionWorkspace && !Terminal && !Editor && !AskCard"),
        ),
        KeyBinding::new(
            "secondary-alt-[",
            PreviousSessionPane,
            Some("SessionWorkspace && !Terminal && !Editor && !AskCard"),
        ),
    ]);
    aether_terminal::bind_keys(cx);
    // Bound after the input's own keys so these win while the menu is open.
    let menu = Some("SuggestionMenu > Input");
    cx.bind_keys([
        KeyBinding::new("up", SuggestionPrev, menu),
        KeyBinding::new("down", SuggestionNext, menu),
        KeyBinding::new("enter", SuggestionAccept, menu),
        KeyBinding::new("tab", SuggestionAccept, menu),
        KeyBinding::new("escape", SuggestionDismiss, menu),
    ]);

    let history = Some("ComposerHistory > Input");
    cx.bind_keys([
        KeyBinding::new("up", ComposerHistoryPrev, history),
        KeyBinding::new("down", ComposerHistoryNext, history),
    ]);
    // Bound after the suggestion keys: that context can wrap this field too, and later wins ties.
    let field_context = format!("{AGENT_SELECTOR_CONTEXT} > Input");
    let field = Some(field_context.as_str());
    let panel = Some(AGENT_SELECTOR_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", SelectorPrev, field),
        KeyBinding::new("down", SelectorNext, field),
        KeyBinding::new("tab", SelectorNextTab, field),
        KeyBinding::new("shift-tab", SelectorPrevTab, field),
        KeyBinding::new("enter", SelectorConfirm, field),
        KeyBinding::new("escape", SelectorDismiss, field),
        KeyBinding::new("enter", SelectorConfirm, panel),
        KeyBinding::new("escape", SelectorDismiss, panel),
    ]);
    // after the input's own keys, or Cmd+Enter would only add a newline
    let composer = format!("{COMPOSER_CONTEXT} > Input");
    let draft = format!("{DRAFT_EDITOR_CONTEXT} > Input");
    cx.bind_keys([
        KeyBinding::new("ctrl-g", EditDraft, Some(composer.as_str())),
        KeyBinding::new("secondary-shift-m", ToggleVoice, Some(COMPOSER_CONTEXT)),
        KeyBinding::new("escape", DraftEditorDone, Some(draft.as_str())),
        KeyBinding::new("ctrl-g", DraftEditorDone, Some(draft.as_str())),
        KeyBinding::new("secondary-s", DraftEditorDone, Some(draft.as_str())),
        KeyBinding::new("secondary-enter", DraftEditorSend, Some(draft.as_str())),
    ]);
    let ask = Some(ASK_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("enter", AskPrimary, ask),
        KeyBinding::new("escape", AskDismiss, ask),
        KeyBinding::new("up", AskPrev, ask),
        KeyBinding::new("down", AskNext, ask),
        KeyBinding::new("space", AskToggle, ask),
    ]);
    let commit = format!("{COMMIT_CONTEXT} > Input");
    cx.bind_keys([KeyBinding::new("secondary-enter", CommitChanges, Some(commit.as_str()))]);
    let choice = Some(CHOICE_MENU_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", ChoiceUp, choice),
        KeyBinding::new("down", ChoiceDown, choice),
        KeyBinding::new("enter", ChoiceConfirm, choice),
        KeyBinding::new("escape", ChoiceDismiss, choice),
    ]);
    // field binding after the input's own keys, so Esc in a form goes back
    let pop = Some(POP_CONTEXT);
    let pop_field = format!("{POP_CONTEXT} > Input");
    let branch_field = format!("{BRANCH_SEARCH_CONTEXT} > Input");
    let publish_field = format!("{PUBLISH_NAME_CONTEXT} > Input");
    let script_field = format!("{SCRIPT_FORM_CONTEXT} > Input");
    cx.bind_keys([
        KeyBinding::new("up", PopUp, pop),
        KeyBinding::new("down", PopDown, pop),
        KeyBinding::new("escape", PopBack, pop),
        KeyBinding::new("escape", PopBack, Some(pop_field.as_str())),
        KeyBinding::new("up", PopUp, Some(branch_field.as_str())),
        KeyBinding::new("down", PopDown, Some(branch_field.as_str())),
        KeyBinding::new("enter", PopConfirm, Some(branch_field.as_str())),
        KeyBinding::new("up", PopUp, Some(publish_field.as_str())),
        KeyBinding::new("down", PopDown, Some(publish_field.as_str())),
        KeyBinding::new("enter", PopConfirm, Some(publish_field.as_str())),
        KeyBinding::new("enter", PopConfirm, Some(script_field.as_str())),
    ]);

    use crate::explorer::{CloseEditorTab, EXPLORER_EDITOR_CONTEXT, NextEditorTab, PrevEditorTab};
    let editor = Some(EXPLORER_EDITOR_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("secondary-w", CloseEditorTab, editor),
        KeyBinding::new("ctrl-tab", NextEditorTab, editor),
        KeyBinding::new("ctrl-shift-tab", PrevEditorTab, editor),
        KeyBinding::new("secondary-shift-]", NextEditorTab, editor),
        KeyBinding::new("secondary-shift-[", PrevEditorTab, editor),
    ]);
    // the editor's Input binds cmd-shift-f to Replace; workspace search wins, replace moves (VS Code)
    let editor_field = format!("{EXPLORER_EDITOR_CONTEXT} > Input");
    cx.bind_keys([
        KeyBinding::new("secondary-shift-f", SearchWorkspace, Some(editor_field.as_str())),
        #[cfg(target_os = "macos")]
        KeyBinding::new(
            "cmd-alt-f",
            gpui_kit::component::input::Replace,
            Some(editor_field.as_str()),
        ),
    ]);
    // menu Home/End; the selector's field binding wins over the input's own Home/End
    cx.bind_keys([
        KeyBinding::new("home", aether_ui::ChoiceFirst, choice),
        KeyBinding::new("end", aether_ui::ChoiceLast, choice),
        KeyBinding::new("home", aether_ui::SelectorFirst, field),
        KeyBinding::new("end", aether_ui::SelectorLast, field),
        KeyBinding::new("home", crate::workspace_popover::PopFirst, pop),
        KeyBinding::new("end", crate::workspace_popover::PopLast, pop),
    ]);
    // after the input's own keys so field keys win; page Esc is shallowest, so popovers' Esc wins
    let branch_select = Some(BRANCH_SELECT_CONTEXT);
    let select_field = format!("{BRANCH_SELECT_CONTEXT} > Input");
    let notes_search = "NotesSearch > Input";
    let quick_open = format!("{QUICK_OPEN_CONTEXT} > Input");
    let generating = format!("{GENERATING_CONTEXT} > Input");
    cx.bind_keys([
        KeyBinding::new("up", SelectPrev, Some(select_field.as_str())),
        KeyBinding::new("down", SelectNext, Some(select_field.as_str())),
        KeyBinding::new("enter", SelectConfirm, Some(select_field.as_str())),
        KeyBinding::new("escape", SelectDismiss, Some(select_field.as_str())),
        KeyBinding::new("up", SelectPrev, branch_select),
        KeyBinding::new("down", SelectNext, branch_select),
        KeyBinding::new("enter", SelectConfirm, branch_select),
        KeyBinding::new("escape", SelectDismiss, branch_select),
        KeyBinding::new("up", NotesSearchPrev, Some(notes_search)),
        KeyBinding::new("down", NotesSearchNext, Some(notes_search)),
        KeyBinding::new("enter", NotesSearchConfirm, Some(notes_search)),
        KeyBinding::new("escape", NotesSearchDismiss, Some(notes_search)),
        KeyBinding::new("escape", NotesSearchDismiss, Some("NotesSearch")),
        KeyBinding::new("up", QuickOpenPrev, Some(quick_open.as_str())),
        KeyBinding::new("down", QuickOpenNext, Some(quick_open.as_str())),
        KeyBinding::new("enter", QuickOpenConfirm, Some(quick_open.as_str())),
        KeyBinding::new("escape", QuickOpenDismiss, Some(quick_open.as_str())),
        KeyBinding::new("escape", StopGenerating, Some(generating.as_str())),
        KeyBinding::new("escape", BackToChat, Some("Aether")),
        KeyBinding::new("secondary-shift-f", SearchWorkspace, None),
    ]);
    // Settings navigation belongs to the sidebar, preserving native editing keys in controls.
    cx.bind_keys([
        KeyBinding::new("up", SettingsPrevious, Some("SettingsSidebar")),
        KeyBinding::new("down", SettingsNext, Some("SettingsSidebar")),
        KeyBinding::new("escape", SettingsDismiss, Some("Settings")),
        KeyBinding::new("escape", SettingsDismiss, Some("Settings > Input")),
        KeyBinding::new("secondary-k", SettingsSearch, Some("SettingsSidebar")),
        KeyBinding::new("escape", SettingsDismiss, Some("SettingsSidebar")),
        KeyBinding::new("secondary-k", SettingsSearch, Some("Settings")),
        KeyBinding::new("secondary-k", SettingsSearch, Some("Settings > Input")),
    ]);
    if cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("cmd-h", Hide, None),
            KeyBinding::new("cmd-alt-h", HideOthers, None),
            KeyBinding::new("cmd-m", Minimize, None),
            // not in the terminal: ctrl-w is readline delete-word there
            KeyBinding::new("ctrl-w", CloseWindow, Some("!Terminal")),
        ]);
    }
}

#[cfg(test)]
#[path = "../tests/unit/actions.rs"]
mod tests;
