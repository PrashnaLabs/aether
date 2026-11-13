use crate::theme::Corners;
use std::path::PathBuf;

use gpui_kit::component::{
    ActiveTheme,
    clipboard::Clipboard,
    h_flex,
    text::{TextView, TextViewState, TextViewStyle},
    v_flex,
};
use gpui_kit::{
    App, Entity, Global, Overflow, Pixels, SharedString, StyleRefinement, Window, div, prelude::*,
    px,
};

use super::cache::RenderCache;
use super::highlight::register_aliases;
use super::links::{link_inline_code, open_link};
use super::tool_row::Reflow;

/// One text block of a reply, rendered as markdown with its fenced code inside.
#[derive(IntoElement)]
pub struct Markdown {
    key: SharedString,
    text: SharedString,
    cwd: Option<SharedString>,
    live: bool,
    muted: bool,
    code_block_max_height: Option<Pixels>,
    on_reflow: Option<Reflow>,
}

impl Markdown {
    pub fn new(key: impl Into<SharedString>, text: SharedString) -> Self {
        Self {
            key: key.into(),
            text,
            cwd: None,
            live: false,
            muted: false,
            code_block_max_height: None,
            on_reflow: None,
        }
    }

    /// Folder inline code paths are linked against; without one, nothing is linked.
    pub fn cwd(mut self, cwd: Option<SharedString>) -> Self {
        self.cwd = cwd.filter(|c| !c.is_empty());
        self
    }

    /// Still streaming: only the raw text is shown, file links wait for the end.
    pub fn live(mut self, live: bool) -> Self {
        self.live = live;
        self
    }

    pub fn muted(mut self, muted: bool) -> Self {
        self.muted = muted;
        self
    }

    /// Limit code blocks inside a constrained container such as a thinking block.
    pub fn code_block_max_height(mut self, height: Pixels) -> Self {
        self.code_block_max_height = Some(height);
        self
    }

    /// Called when the text's height may have changed, e.g. a large text
    /// finishing its background parse.
    pub fn on_reflow(mut self, on_reflow: Option<Reflow>) -> Self {
        self.on_reflow = on_reflow;
        self
    }
}

impl RenderOnce for Markdown {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let view = markdown(&self, cx);
        v_flex().w_full().when(self.muted, |d| d.text_color(muted)).children(view)
    }
}

// Owned, not keyed: keyed state dies offscreen and >4KB re-parses async, so rows measured empty.
/// Per block: its source, that source with file links once resolved, whether that
/// resolve is running, its view state, and the reflow its latest render passed.
struct MarkdownEntry {
    raw: String,
    linked: Option<SharedString>,
    /// Last source pushed into the view: re-setting the view re-parses it.
    shown: String,
    linking: bool,
    state: Entity<TextViewState>,
    reflow: Option<Reflow>,
}

struct MarkdownStates(RenderCache<MarkdownEntry>);

impl Default for MarkdownStates {
    fn default() -> Self {
        Self(RenderCache::new(512))
    }
}

impl Global for MarkdownStates {}

fn wants_link(live: bool, unlinked: bool, linking: bool, text: &str, has_cwd: bool) -> bool {
    !live && unlinked && !linking && has_cwd && text.contains('`')
}

const VIEW_CAP: usize = 256 * 1024;

// text view layout grows ~quadratically: a 1 MB message would stall the UI ~13 s
pub(crate) fn clip_for_view(text: &str) -> std::borrow::Cow<'_, str> {
    if text.len() <= VIEW_CAP {
        return text.into();
    }
    let mut cut = VIEW_CAP;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let cut = text[..cut].rfind('\n').filter(|&i| i > VIEW_CAP / 2).unwrap_or(cut);
    let more = (text.len() - cut).div_ceil(1024);
    format!("{}\n\n*… message truncated in view ({more} KB more)*", &text[..cut]).into()
}

fn markdown(md: &Markdown, cx: &mut App) -> Option<TextView> {
    register_aliases();
    let clipped = clip_for_view(md.text.trim());
    let text: &str = &clipped;
    if text.is_empty() {
        return None;
    }
    let key = &md.key;
    let cache = &mut cx.default_global::<MarkdownStates>().0;
    let state = match cache.get(key) {
        Some(entry) => {
            // rows shift as replies fold, so the parse observer calls the newest reflow
            entry.reflow = md.on_reflow.clone();
            if entry.raw != text {
                entry.raw = text.to_string();
                entry.linked = None;
            }
            entry.state.clone()
        }
        None => {
            let state = cx.new(|cx| TextViewState::markdown(text, cx));
            let observed = key.clone();
            cx.observe(&state, move |_, cx| {
                let entry = cx.default_global::<MarkdownStates>().0.get(&observed);
                if let Some(reflow) = entry.and_then(|entry| entry.reflow.clone()) {
                    reflow(cx);
                }
            })
            .detach();
            let entry = MarkdownEntry {
                raw: text.to_string(),
                linked: None,
                shown: text.to_string(),
                linking: false,
                state: state.clone(),
                reflow: md.on_reflow.clone(),
            };
            cx.default_global::<MarkdownStates>().0.insert(key.clone(), entry);
            state
        }
    };
    let entry = cx.default_global::<MarkdownStates>().0.get(key)?;
    let shown = entry.linked.clone();
    let link_wanted = wants_link(md.live, shown.is_none(), entry.linking, text, md.cwd.is_some());
    if let Some(cwd) = md.cwd.clone().filter(|_| link_wanted) {
        entry.linking = true;
        let (key, raw, state) = (key.clone(), text.to_string(), state.clone());
        cx.spawn(async move |cx| {
            let source = raw.clone();
            let linked = cx
                .background_executor()
                .spawn(async move { link_inline_code(&source, cwd.as_ref().as_ref()) })
                .await;
            cx.update(|cx| {
                let Some(entry) = cx.default_global::<MarkdownStates>().0.get(&key) else {
                    return;
                };
                entry.linking = false;
                let reflow = entry.reflow.clone();
                if entry.raw == raw {
                    let linked = SharedString::from(linked);
                    entry.linked = Some(linked.clone());
                    entry.shown = linked.to_string();
                    state.update(cx, |state, cx| state.set_text(&linked, cx));
                }
                if let Some(reflow) = reflow {
                    reflow(cx);
                }
            });
        })
        .detach();
    }
    let source = shown.unwrap_or_else(|| text.to_string().into());
    let changed = match cx.default_global::<MarkdownStates>().0.get(key) {
        Some(entry) => {
            if entry.shown == source {
                false
            } else {
                entry.shown = source.to_string();
                true
            }
        }
        None => false,
    };
    if changed {
        state.update(cx, |state, cx| state.set_text(&source, cx));
    }

    let theme = cx.theme();
    let (muted, mono) = (theme.muted_foreground, theme.mono_font_family.clone());
    let code = code_block_style(mono, md.code_block_max_height);
    let cwd = md.cwd.as_deref().map(PathBuf::from);
    Some(
        TextView::new(&state)
            .selectable(true)
            .style(TextViewStyle::default().code_block(code))
            .code_block_actions(move |block, _, _| {
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        div().text_xs().text_color(muted).child(block.lang().unwrap_or_default()),
                    )
                    .child(Clipboard::new("copy").value(block.code()).tooltip("Copy code"))
            })
            .on_link_click(move |href, ev, window, cx| {
                if !ev.is_right_click() {
                    open_link(href, cwd.as_deref(), window, cx);
                }
            }),
    )
}

fn code_block_style(mono: SharedString, max_height: Option<Pixels>) -> StyleRefinement {
    let mut style = StyleRefinement::default().corner(px(9.)).pt(px(32.)).font_family(mono);
    if let Some(height) = max_height {
        style = style.max_h(height);
        style.overflow.y = Some(Overflow::Scroll);
    }
    style
}

#[cfg(test)]
#[path = "../../tests/unit/chat/markdown.rs"]
mod tests;
