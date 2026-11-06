use std::collections::{HashMap, HashSet};
use std::ops::Range;

use gpui_kit::{
    App, Context, Entity, FocusHandle, KeyDownEvent, ListAlignment, ListOffset, ListState,
    SharedString, Window, px,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Nav {
    Up,
    Down,
    Home,
    End,
}

/// Row that arrow keys move focus to, skipping section captions.
fn target_row(focusable: &[bool], from: usize, nav: Nav) -> Option<usize> {
    let n = focusable.len();
    match nav {
        Nav::Down => (from + 1..n).find(|&i| focusable[i]),
        Nav::Up => (0..from.min(n)).rev().find(|&i| focusable[i]),
        Nav::Home => (0..n).find(|&i| focusable[i]),
        Nav::End => (0..n).rev().find(|&i| focusable[i]),
    }
}

/// The old rows to replace and how many new rows replace them; rows shared
/// at both ends stay, keeping their measured heights.
pub fn splice_range<T: PartialEq>(old: &[T], new: &[T]) -> (Range<usize>, usize) {
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    (prefix..old.len() - suffix, new.len() - prefix - suffix)
}

/// Sidebar state that outlives a frame: the virtual list, one focus handle
/// per focusable row keyed by what the row shows, and the open row menu.
pub(super) struct SidebarState {
    pub(super) list: ListState,
    keys: Vec<SharedString>,
    pub(super) focus: HashMap<SharedString, FocusHandle>,
    pub(super) open_menu: Option<SharedString>,
    pub(super) collapsed_crews: HashSet<i64>,
    /// Row last scrolled into view for being selected or renamed.
    revealed: Option<SharedString>,
}

impl SidebarState {
    pub(super) fn new(window: &Window) -> Self {
        let overdraw = px(window.viewport_size().height.as_f32() * 0.3);
        Self {
            list: ListState::new(0, ListAlignment::Top, overdraw),
            keys: Vec::new(),
            focus: HashMap::new(),
            open_menu: None,
            collapsed_crews: HashSet::new(),
            revealed: None,
        }
    }

    /// Splices only the rows that changed, so the list never jumps to the top;
    /// the row at the top of the view stays there unless the list is at its top.
    pub(super) fn sync(
        &mut self,
        keys: Vec<SharedString>,
        focusable: &[bool],
        cx: &mut Context<Self>,
    ) {
        if keys == self.keys {
            return;
        }
        let live: HashSet<&SharedString> = keys.iter().collect();
        self.focus.retain(|key, _| live.contains(key));
        for (key, _) in keys.iter().zip(focusable).filter(|(_, f)| **f) {
            self.focus.entry(key.clone()).or_insert_with(|| cx.focus_handle().tab_stop(true));
        }
        let top = self.list.logical_scroll_top();
        let anchor = (top.item_ix > 0 || top.offset_in_item > px(0.))
            .then(|| self.keys.get(top.item_ix).cloned())
            .flatten();
        let (old, count) = splice_range(&self.keys, &keys);
        let start = old.start;
        let handles: Vec<Option<FocusHandle>> =
            keys[start..start + count].iter().map(|key| self.focus.get(key).cloned()).collect();
        self.list.splice_focusable(old, handles);
        if let Some(ix) = anchor.and_then(|a| keys.iter().position(|key| *key == a)) {
            self.list.scroll_to(ListOffset { item_ix: ix, offset_in_item: top.offset_in_item });
        }
        self.keys = keys;
    }

    pub(super) fn reveal(&mut self, key: Option<SharedString>) {
        if key == self.revealed {
            return;
        }
        if let Some(ix) = key.as_ref().and_then(|k| self.keys.iter().position(|x| x == k)) {
            self.list.scroll_to_reveal_item(ix);
        }
        self.revealed = key;
    }
}

/// Moves focus with Up/Down/Home/End, scrolling the target row into view.
/// Returns whether the key was one of those.
pub(super) fn navigate(
    state: &Entity<SidebarState>,
    key: &SharedString,
    ev: &KeyDownEvent,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let nav = match ev.keystroke.key.as_str() {
        "up" => Nav::Up,
        "down" => Nav::Down,
        "home" => Nav::Home,
        "end" => Nav::End,
        _ => return false,
    };
    if ev.keystroke.modifiers.modified() {
        return false;
    }
    let s = state.read(cx);
    let focusable: Vec<bool> = s.keys.iter().map(|k| s.focus.contains_key(k)).collect();
    let target = s
        .keys
        .iter()
        .position(|k| k == key)
        .and_then(|from| target_row(&focusable, from, nav))
        .map(|to| {
            s.list.scroll_to_reveal_item(to);
            s.focus[&s.keys[to]].clone()
        });
    if let Some(handle) = target {
        handle.focus(window, cx);
    }
    true
}

#[cfg(test)]
#[path = "../../../tests/unit/chat/sidebar/state.rs"]
mod tests;
