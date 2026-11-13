use aether_ui::theme::Corners;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use aether_ui::{field, field_area, overlay, overlay_strong, provider_icon};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Icon, Sizable, checkbox::Checkbox, h_flex, v_flex};
use gpui_kit::{
    AnyElement, ClickEvent, Context, Entity, FontWeight, Hsla, Role, SharedString, Subscription,
    Window, div, prelude::*, px,
};

use super::model::{BoardCard, Col, preview, shown};
use crate::app::{Chat, ChatApp, now_ms};
use crate::project_page::model::{CardStatus, badge, card_status};
use crate::project_page::view::{pill, status_tint};
use crate::projects::relative_time;

#[derive(Clone)]
struct CardDrag {
    id: i64,
    title: SharedString,
}

impl Render for CardDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .w(px(240.))
            .px(px(10.))
            .py(px(8.))
            .corner(px(8.))
            .border_1()
            .border_color(theme.ring)
            .bg(theme.background)
            .shadow_md()
            .text_size(px(13.))
            .font_weight(FontWeight::MEDIUM)
            .truncate()
            .child(self.title.clone())
    }
}

fn col_icon(col: Col) -> Lucide {
    match col {
        Col::Backlog => Lucide::Circle,
        Col::Todo => Lucide::CircleDot,
        Col::Doing => Lucide::LoaderCircle,
        Col::Review => Lucide::Eye,
        Col::Done => Lucide::CircleCheck,
    }
}

fn folder_name(project: &str) -> String {
    Path::new(project).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

fn meta_row(label: &'static str, value: impl IntoElement, muted: Hsla) -> impl IntoElement {
    h_flex()
        .gap(px(8.))
        .min_h(px(24.))
        .items_center()
        .text_size(px(12.5))
        .child(div().w(px(64.)).flex_none().text_color(muted).child(label))
        .child(div().flex_1().min_w_0().child(value))
}

/// Detail panel inputs; keyed window state, since ChatApp has no fields for them.
struct Editors {
    labels: Entity<InputState>,
    blockers: Entity<InputState>,
    note: Entity<InputState>,
    /// card id, labels text, blockers text last pushed into the inputs
    synced: (i64, String, String),
    expanded: HashSet<usize>,
    _subs: Vec<Subscription>,
}

type Apply = fn(&mut ChatApp, i64, &str, &mut Context<ChatApp>);

impl Editors {
    fn new(app: Entity<ChatApp>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut input =
            |hint: &'static str| cx.new(|cx| InputState::new(window, cx).placeholder(hint));
        let labels = input("bug, ui, backend");
        let blockers = input("#12, #15");
        let note = input("Add a note, Enter to save");
        // labels and blockers commit on enter or blur; a note only on enter, then clears
        let wire =
            |cx: &mut Context<Self>, input: &Entity<InputState>, apply: Apply, is_note: bool| {
                let app = app.clone();
                cx.subscribe_in(input, window, move |_, input, ev: &InputEvent, window, cx| {
                    let go = match ev {
                        InputEvent::PressEnter { .. } => true,
                        InputEvent::Blur => !is_note,
                        _ => false,
                    };
                    if !go {
                        return;
                    }
                    let text = input.read(cx).value().to_string();
                    if is_note {
                        input.update(cx, |s, cx| s.set_value(String::new(), window, cx));
                    }
                    app.update(cx, |this, cx| {
                        if let Some(id) = this.board_selected {
                            apply(this, id, &text, cx);
                        }
                    });
                })
            };
        let _subs = vec![
            wire(cx, &labels, ChatApp::board_set_labels, false),
            wire(cx, &blockers, ChatApp::board_set_blockers, false),
            wire(cx, &note, ChatApp::board_add_note, true),
        ];
        Self {
            labels,
            blockers,
            note,
            synced: (0, String::new(), String::new()),
            expanded: HashSet::new(),
            _subs,
        }
    }
}

fn chip(text: String, bg: Hsla, fg: Hsla) -> impl IntoElement {
    div()
        .flex_none()
        .px(px(6.))
        .h(px(18.))
        .flex()
        .items_center()
        .corner(px(5.))
        .bg(bg)
        .text_color(fg)
        .text_size(px(11.5))
        .child(text)
}

fn id_list(ids: &[i64]) -> String {
    ids.iter().map(|id| format!("#{id}")).collect::<Vec<_>>().join(", ")
}

impl ChatApp {
    fn board_chat(&self, card: &BoardCard) -> Option<&Chat> {
        card.chats
            .iter()
            .rev()
            .find_map(|id| self.chats.iter().find(|c| c.id == *id && !c.archived))
    }

    /// Keeps the inputs showing the selected card, also after a lead edits it.
    fn board_editors(&self, window: &mut Window, cx: &mut Context<Self>) -> Entity<Editors> {
        let app = cx.entity();
        let ed = window
            .use_keyed_state("board-editors", cx, move |window, cx| Editors::new(app, window, cx));
        if let Some(card) = self.cards.iter().find(|c| Some(c.id) == self.board_selected) {
            let want = (card.id, card.labels.join(", "), id_list(&card.blocked_by));
            if ed.read(cx).synced != want {
                let (labels, blockers, note) = {
                    let e = ed.read(cx);
                    (e.labels.clone(), e.blockers.clone(), e.note.clone())
                };
                let switched = ed.read(cx).synced.0 != want.0;
                labels.update(cx, |s, cx| s.set_value(want.1.clone(), window, cx));
                blockers.update(cx, |s, cx| s.set_value(want.2.clone(), window, cx));
                if switched {
                    note.update(cx, |s, cx| s.set_value(String::new(), window, cx));
                }
                ed.update(cx, |e, _| {
                    if switched {
                        e.expanded.clear();
                    }
                    e.synced = want;
                });
            }
        }
        ed
    }

    fn board_pill(&self, card: &BoardCard, now: i64, cx: &Context<Self>) -> Option<AnyElement> {
        let status = self.board_status(card, now)?;
        let since = self.board_chat(card).map_or(now, |c| c.turn_at);
        let (bg, fg) = status_tint(status, cx);
        badge(status, since, now).map(|text| pill(text, bg, fg).into_any_element())
    }

    pub(crate) fn render_board(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let lights = aether_ui::traffic_lights_gap(self.settings.sidebar_collapsed, window, cx);
        let top = (lights, aether_ui::window_controls_gap(window));
        self.board_view(None, Some(top), Some(window), cx)
    }

    pub(crate) fn render_board_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.board_view(self.chat_project(self.active), None, Some(window), cx)
    }

    fn board_view(
        &mut self,
        project: Option<PathBuf>,
        window_top: Option<(gpui_kit::Stateful<gpui_kit::Div>, gpui_kit::Div)>,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now = now_ms();
        let editors = window.map(|window| self.board_editors(window, cx));
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let scope = project.as_ref().filter(|_| !self.board_all).map(|d| d.display().to_string());
        let query = self.board_search.read(cx).value().to_string();
        let visible: Vec<&BoardCard> = self
            .cards
            .iter()
            .filter(|c| scope.as_ref().is_none_or(|p| c.project == *p))
            .filter(|c| shown(c, &query, self.board_by))
            .collect();
        let open = visible.iter().filter(|c| c.col != Col::Done).count();
        let (mut running, mut waiting) = (0, 0);
        for card in &visible {
            match self.board_status(card, now) {
                Some(CardStatus::Working) => running += 1,
                Some(CardStatus::Waiting(_)) => waiting += 1,
                _ => {}
            }
        }
        let mut summary = vec![format!("{open} open")];
        if running > 0 {
            summary.push(format!("{running} running"));
        }
        if waiting > 0 {
            summary.push(format!("{waiting} need{} you", if waiting == 1 { "s" } else { "" }));
        }
        let title: SharedString = match &project {
            Some(dir) if scope.is_some() => self.project_name(dir),
            _ => "Tasks board".into(),
        };
        let switch = project.is_some().then(|| {
            let all = self.board_all;
            let app = cx.entity();
            aether_ui::button("board-scope")
                .ghost()
                .small()
                .label(if all { "All projects" } else { "This project" })
                .dropdown_caret(true)
                .accessibility_label("Board project scope")
                .dropdown_menu(move |mut menu, _, _| {
                    for (value, label) in [(false, "This project"), (true, "All projects")] {
                        let app = app.clone();
                        menu = menu.item(PopupMenuItem::new(label).checked(all == value).on_click(
                            move |_, _, cx| {
                                app.update(cx, |this, cx| {
                                    this.board_all = value;
                                    cx.notify();
                                })
                            },
                        ));
                    }
                    menu
                })
        });
        let by = self.board_by;
        let app = cx.entity();
        let authors = aether_ui::button("board-author")
            .ghost()
            .small()
            .label(match by {
                None => "All",
                Some(false) => "Mine",
                Some(true) => "Agents",
            })
            .dropdown_caret(true)
            .accessibility_label("Cards added by")
            .dropdown_menu(move |mut menu, _, _| {
                for (value, label) in [(None, "All"), (Some(false), "Mine"), (Some(true), "Agents")]
                {
                    let app = app.clone();
                    menu = menu.item(PopupMenuItem::new(label).checked(by == value).on_click(
                        move |_, _, cx| {
                            app.update(cx, |this, cx| {
                                this.board_by = value;
                                cx.notify();
                            })
                        },
                    ));
                }
                menu
            });
        let search = div().w(px(200.)).child(
            field(&self.board_search)
                .small()
                .icon(gpui_kit::component::IconName::Search)
                .aria_label("Search cards"),
        );
        let (lights, controls_gap) = window_top.unzip();
        let header = h_flex()
            .children(lights)
            .px(px(20.))
            .pt(px(16.))
            .pb(px(14.))
            .gap(px(12.))
            .items_center()
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(10.))
                    .items_baseline()
                    .child(
                        div()
                            .id("board-title")
                            .role(Role::Heading)
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(muted)
                            .truncate()
                            .child(summary.join(" · ")),
                    ),
            )
            .child(search)
            .child(authors)
            .children(switch)
            .child(
                aether_ui::button("board-new-card")
                    .primary()
                    .small()
                    .h(px(28.))
                    .icon(Lucide::Plus)
                    .label("New card")
                    .tooltip("Add a card to Todo")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.board_new_card(Col::Todo, window, cx)
                    })),
            )
            .children(controls_gap);
        let all: Vec<i64> = visible.iter().map(|c| c.id).collect();
        let bulk = self.board_bulk_bar(all, cx);
        let empty = visible.is_empty();
        let columns = Col::ALL.map(|col| self.board_column(col, scope.as_deref(), empty, now, cx));
        let detail = self.board_detail(now, editors.as_ref(), cx);
        v_flex()
            .id("board-page")
            .flex_1()
            .min_h_0()
            .child(header)
            .children(bulk)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .px(px(16.))
                    .pb(px(16.))
                    .gap(px(12.))
                    .child(
                        h_flex()
                            .id("board-columns")
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .items_stretch()
                            .gap(px(8.))
                            .overflow_x_scroll()
                            .restrict_scroll_to_axis()
                            .children(columns),
                    )
                    .children(detail),
            )
            .into_any_element()
    }

    fn board_column(
        &self,
        col: Col,
        scope: Option<&str>,
        board_empty: bool,
        now: i64,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let query = self.board_search.read(cx).value().to_string();
        let theme = cx.theme();
        let (fg, muted, ring) = (theme.foreground, theme.muted_foreground, theme.ring);
        let (lane, hover) = (overlay(cx), overlay_strong(cx));
        let tint = match col {
            Col::Backlog => muted,
            Col::Todo => fg,
            Col::Doing => aether_ui::theme::mango_text(cx),
            Col::Review => aether_ui::theme::info_text(cx),
            Col::Done => aether_ui::theme::success_text(cx),
        };
        let mut list: Vec<&BoardCard> = self
            .cards
            .iter()
            .filter(|c| c.col == col && scope.is_none_or(|p| c.project == p))
            .filter(|c| shown(c, &query, self.board_by))
            .collect();
        list.sort_by(|a, b| a.position.total_cmp(&b.position));
        let n = list.len();
        let cards: Vec<_> = list.into_iter().map(|card| self.board_card(card, now, cx)).collect();
        let hint = (board_empty && col == Col::Todo).then(|| {
            div()
                .px(px(8.))
                .py(px(6.))
                .text_size(px(12.5))
                .line_height(px(17.))
                .text_color(muted)
                .child("No cards yet. Add one here, or let a lead agent create them.")
        });
        let add = (col != Col::Done).then(|| {
            h_flex()
                .id(("board-add", col as usize))
                .role(Role::Button)
                .tab_index(0)
                .aria_label(format!("Add card to {}", col.label()))
                .flex_none()
                .h(px(32.))
                .px(px(8.))
                .gap(px(6.))
                .items_center()
                .corner(px(8.))
                .text_size(px(12.5))
                .text_color(muted)
                .cursor_pointer()
                .hover(move |s| s.bg(hover).text_color(fg))
                .focus_visible(move |s| s.bg(hover).text_color(fg))
                .child(Icon::new(Lucide::Plus).size(px(14.)))
                .child("Add card")
                .on_click(
                    cx.listener(move |this, _, window, cx| this.board_new_card(col, window, cx)),
                )
        });
        v_flex()
            .id(("board-col", col as usize))
            .role(Role::List)
            .aria_label(col.label())
            .flex_none()
            .w(px(288.))
            .h_full()
            .min_h_0()
            .p(px(6.))
            .corner(px(12.))
            .bg(lane)
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .drag_over::<CardDrag>(move |s, _, _, _| s.border_color(ring))
            .on_drop(cx.listener(move |this, drag: &CardDrag, window, cx| {
                this.board_move(drag.id, col, None);
                this.org_flush_wake(window, cx);
                cx.notify();
            }))
            .child(
                h_flex()
                    .flex_none()
                    .h(px(32.))
                    .px(px(8.))
                    .gap(px(8.))
                    .items_center()
                    .child(Icon::new(col_icon(col)).size(px(14.)).text_color(tint))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(col.label()),
                    )
                    .child(
                        div()
                            .min_w(px(20.))
                            .h(px(18.))
                            .px(px(6.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .corner(px(6.))
                            .bg(hover)
                            .text_size(px(11.5))
                            .text_color(muted)
                            .child(n.to_string()),
                    ),
            )
            .child(
                v_flex()
                    .id(("board-col-cards", col as usize))
                    .flex_1()
                    .min_h_0()
                    .pt(px(2.))
                    .gap(px(6.))
                    .overflow_y_scroll()
                    .restrict_scroll_to_axis()
                    .children(cards)
                    .children(hint)
                    .children(add),
            )
    }

    fn board_bulk_bar(&self, all: Vec<i64>, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picked: Vec<i64> =
            all.iter().copied().filter(|id| self.board_picks.contains(id)).collect();
        if picked.is_empty() {
            return None;
        }
        let n = picked.len();
        let select_all = (n < all.len()).then(|| {
            aether_ui::button("board-pick-all").ghost().xsmall().label("Select all").on_click(
                cx.listener(move |this, _, _, cx| {
                    this.board_picks.extend(all.iter().copied());
                    cx.notify();
                }),
            )
        });
        let to_done = aether_ui::button("board-pick-done")
            .small()
            .h(px(28.))
            .outline()
            .icon(Lucide::CircleCheck)
            .label("Move to Done")
            .tooltip("Accept the selected cards; cards waiting on them are unblocked")
            .on_click(cx.listener({
                let picked = picked.clone();
                move |this, _, _, cx| {
                    for id in &picked {
                        this.board_move(*id, Col::Done, None);
                    }
                    this.board_picks.clear();
                    cx.notify();
                }
            }));
        let delete = aether_ui::button("board-pick-delete")
            .small()
            .h(px(28.))
            .danger()
            .icon(Lucide::Trash)
            .label("Delete")
            .on_click(cx.listener(move |this, _, window, cx| {
                this.confirm_delete_cards(picked.clone(), None, window, cx)
            }));
        let clear = aether_ui::button("board-pick-clear")
            .ghost()
            .xsmall()
            .icon(Lucide::X)
            .accessibility_label("Clear selection")
            .tooltip("Clear selection")
            .on_click(cx.listener(|this, _, _, cx| {
                this.board_picks.clear();
                cx.notify();
            }));
        let theme = cx.theme();
        Some(
            h_flex()
                .id("board-bulk")
                .role(Role::Toolbar)
                .aria_label("Selected cards")
                .mx(px(16.))
                .mb(px(10.))
                .gap(px(6.))
                .items_center()
                .p(px(6.))
                .corner(px(9.))
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .child(
                    div()
                        .px(px(4.))
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .child(format!("{n} selected")),
                )
                .children(select_all)
                .child(div().flex_1())
                .child(to_done)
                .child(delete)
                .child(clear)
                .into_any_element(),
        )
    }

    fn board_card(
        &self,
        card: &BoardCard,
        now: i64,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let (fg, muted, ring) = (theme.foreground, theme.muted_foreground, theme.ring);
        let (hover, pressed) = (muted.opacity(0.35), overlay_strong(cx));
        let selected = self.board_selected == Some(card.id);
        let picked = self.board_picks.contains(&card.id);
        let picking = !self.board_picks.is_empty();
        let border = if selected || picked { ring } else { theme.border };
        let done = card.col == Col::Done;
        let status = self.board_pill(card, now, cx);
        let chat = self.board_chat(card);
        let agent = chat.map(|c| provider_icon(c.provider).size(px(12.)));
        let folder = folder_name(&card.project);
        let age = (status.is_none()).then(|| relative_time(card.updated_at, now, &chrono::Local));
        let (id, col) = (card.id, card.col);
        let drag = CardDrag { id, title: card.title.clone().into() };
        let footer = (agent.is_some() || !folder.is_empty() || status.is_some() || age.is_some())
            .then(|| {
                h_flex()
                    .gap(px(6.))
                    .items_center()
                    .min_h(px(20.))
                    .text_size(px(12.))
                    .text_color(muted)
                    .children(agent)
                    .when(!folder.is_empty(), |d| {
                        d.child(Icon::new(Lucide::Folder).size(px(12.)))
                            .child(div().min_w_0().truncate().child(folder))
                    })
                    .child(div().flex_1())
                    .children(status)
                    .children(age.map(|a| div().flex_none().child(a)))
            });
        let (tag_bg, danger) = (overlay_strong(cx), aether_ui::theme::danger_text(cx));
        let marks = (!card.labels.is_empty()
            || !card.blocked_by.is_empty()
            || card.chats.len() > 1
            || !card.notes.is_empty())
        .then(|| {
            let stat = |icon: Lucide, text: String| {
                h_flex()
                    .gap(px(3.))
                    .items_center()
                    .flex_none()
                    .child(Icon::new(icon).size(px(11.)))
                    .child(text)
            };
            h_flex()
                .flex_wrap()
                .gap(px(4.))
                .items_center()
                .text_size(px(11.5))
                .text_color(muted)
                .when(!card.blocked_by.is_empty(), |d| {
                    d.child(
                        h_flex()
                            .gap(px(3.))
                            .items_center()
                            .flex_none()
                            .text_color(danger)
                            .child(Icon::new(Lucide::Ban).size(px(11.)))
                            .child(format!("blocked {}", id_list(&card.blocked_by))),
                    )
                })
                .children(card.labels.iter().map(|l| chip(l.clone(), tag_bg, muted)))
                .when(card.chats.len() > 1, |d| {
                    d.child(stat(Lucide::Bot, format!("{} agents", card.chats.len())))
                })
                .when(!card.notes.is_empty(), |d| {
                    d.child(stat(Lucide::MessageSquare, card.notes.len().to_string()))
                })
        });
        let group = SharedString::from(format!("board-card-{id}"));
        let pick = div()
            .id(("board-card-pick-slot", id as u64))
            .absolute()
            .top(px(8.))
            .right(px(8.))
            .when(!picking, |d| d.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.)))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                Checkbox::new(("board-card-pick", id as u64))
                    .checked(picked)
                    .tab_stop(false)
                    .aria_label(SharedString::from(format!("Select {}", card.title)))
                    .on_click(cx.listener(move |this, _: &bool, _, cx| {
                        this.board_toggle_pick(id);
                        cx.notify();
                    })),
            );
        v_flex()
            .id(("board-card", id as u64))
            .group(group)
            .relative()
            .role(Role::Button)
            .tab_index(0)
            .aria_label(card.title.clone())
            .px(px(10.))
            .py(px(9.))
            .gap(px(6.))
            .corner(px(8.))
            .border_1()
            .border_color(border)
            .bg(theme.background)
            .shadow_xs()
            .cursor_pointer()
            .when(!selected, |d| d.hover(move |s| s.border_color(hover)))
            .focus_visible(move |s| s.border_color(ring))
            .active(move |s| s.bg(pressed))
            .drag_over::<CardDrag>(move |s, _, _, _| s.border_color(ring))
            .on_drag(drag, |d: &CardDrag, _, _, cx| cx.new(|_| d.clone()))
            .on_drop(cx.listener(move |this, drag: &CardDrag, window, cx| {
                if drag.id != id {
                    this.board_move(drag.id, col, Some(id));
                    this.org_flush_wake(window, cx);
                    cx.notify();
                }
            }))
            .child(
                div()
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if done { muted } else { fg })
                    .line_clamp(2)
                    .pr(px(18.))
                    .child(card.title.clone()),
            )
            .when(!card.body.trim().is_empty() && !done, |d| {
                d.child(
                    div()
                        .text_size(px(12.5))
                        .line_height(px(17.))
                        .text_color(muted)
                        .line_clamp(4)
                        .child(preview(&card.body, 4)),
                )
            })
            .children(marks)
            .children(footer)
            .child(pick)
            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                let m = ev.modifiers();
                if m.platform || m.shift || !this.board_picks.is_empty() {
                    this.board_toggle_pick(id);
                    cx.notify();
                } else {
                    this.board_pick(id, window, cx);
                }
            }))
    }

    fn board_detail(
        &self,
        now: i64,
        editors: Option<&Entity<Editors>>,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let card = self.cards.iter().find(|c| Some(c.id) == self.board_selected)?;
        let inputs = editors.map(|e| {
            let e = e.read(cx);
            (e.labels.clone(), e.blockers.clone(), e.note.clone(), e.expanded.clone())
        });
        let editors = editors.cloned();
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let id = card.id;
        let chat = self.board_chat(card);
        let status = self.board_pill(card, now, cx);
        let col_chip = h_flex()
            .gap(px(6.))
            .items_center()
            .text_size(px(12.5))
            .text_color(muted)
            .child(Icon::new(col_icon(card.col)).size(px(14.)))
            .child(card.col.label());
        let close = aether_ui::button("board-close")
            .ghost()
            .xsmall()
            .icon(Lucide::X)
            .accessibility_label("Close card")
            .tooltip("Close")
            .on_click(cx.listener(|this, _, window, cx| {
                this.board_commit_in(window, cx);
                this.board_selected = None;
                cx.notify();
            }));
        let label = |text: &'static str| {
            div().text_size(px(12.5)).text_color(muted).mb(px(-4.)).child(text)
        };
        let folder = folder_name(&card.project);
        let project = (!folder.is_empty()).then(|| {
            meta_row(
                "Project",
                h_flex()
                    .id("board-detail-project")
                    .gap(px(6.))
                    .items_center()
                    .child(Icon::new(Lucide::Folder).size(px(12.)).text_color(muted))
                    .child(div().min_w_0().truncate().child(folder))
                    .tooltip({
                        let path: SharedString = card.project.clone().into();
                        move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(path.clone())
                                .build(window, cx)
                        }
                    }),
                muted,
            )
        });
        let (labels_in, blockers_in, note_in, expanded) = match inputs {
            Some((l, b, n, x)) => (Some(l), Some(b), Some(n), x),
            None => (None, None, None, HashSet::new()),
        };
        let (fg, tag_bg, lane) = (theme.foreground, overlay_strong(cx), overlay(cx));
        let chips = (!card.labels.is_empty()).then(|| {
            h_flex()
                .flex_wrap()
                .gap(px(4.))
                .children(card.labels.iter().map(|l| chip(l.clone(), tag_bg, fg)))
        });
        let blockers: Vec<_> = card
            .blocked_by
            .iter()
            .filter_map(|b| self.cards.iter().find(|c| c.id == *b))
            .map(|b| {
                let bid = b.id;
                h_flex()
                    .id(("board-blocker", bid as u64))
                    .role(Role::Button)
                    .tab_index(0)
                    .gap(px(6.))
                    .items_center()
                    .min_h(px(22.))
                    .text_size(px(12.5))
                    .cursor_pointer()
                    .hover(move |s| s.text_color(fg))
                    .child(Icon::new(Lucide::Ban).size(px(12.)).text_color(muted))
                    .child(div().min_w_0().truncate().child(format!("#{bid} {}", b.title)))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.board_pick(bid, window, cx)),
                    )
            })
            .collect();
        let agents: Vec<_> = card
            .chats
            .iter()
            .filter_map(|cid| self.chats.iter().find(|c| c.id == *cid && !c.archived))
            .map(|c| {
                let cid = c.id;
                let status = card_status(
                    c.status(now),
                    c.messages.last().is_some_and(|m| m.failed),
                    c.wait_label(),
                );
                let (bg, tint) = status_tint(status, cx);
                let state = badge(status, c.turn_at, now).map(|t| pill(t, bg, tint));
                h_flex()
                    .gap(px(6.))
                    .items_center()
                    .min_h(px(26.))
                    .text_size(px(12.5))
                    .child(provider_icon(c.provider).size(px(12.)))
                    .child(div().flex_1().min_w_0().truncate().child(c.title.clone()))
                    .children(state)
                    .child(
                        aether_ui::button(("board-agent-open", cid as u64))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::MessageSquare)
                            .accessibility_label("Open chat")
                            .tooltip("Open chat")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.board_open_chat(cid, window, cx)
                            })),
                    )
            })
            .collect();
        let notes: Vec<_> = card
            .notes
            .iter()
            .enumerate()
            .rev()
            .map(|(ix, n)| {
                let long = editors.is_some()
                    && (n.text.lines().count() > 6 || n.text.chars().count() > 400);
                let open = expanded.contains(&ix);
                let who = if n.by == "user" { "You".to_string() } else { n.by.clone() };
                let more = long.then(|| {
                    let editors = editors.clone();
                    aether_ui::button(("board-note-more", ix as u64))
                        .ghost()
                        .xsmall()
                        .label(if open { "Show less" } else { "Show more" })
                        .on_click(cx.listener(move |_, _, _, cx| {
                            if let Some(ed) = &editors {
                                ed.update(cx, |e, _| {
                                    if !e.expanded.remove(&ix) {
                                        e.expanded.insert(ix);
                                    }
                                });
                            }
                            cx.notify();
                        }))
                });
                v_flex()
                    .w_full()
                    .min_w_0()
                    .overflow_hidden()
                    .gap(px(3.))
                    .p(px(8.))
                    .corner(px(8.))
                    .bg(lane)
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .text_size(px(11.5))
                            .text_color(muted)
                            .child(div().font_weight(FontWeight::MEDIUM).child(who))
                            .child(relative_time(n.at, now, &chrono::Local)),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .text_size(px(12.5))
                            .line_height(px(17.))
                            .when(long && !open, |d| d.line_clamp(6))
                            .child(n.text.clone()),
                    )
                    .children(more)
            })
            .collect();
        let unlinked = !card.chats.is_empty() && chat.is_none();
        let gone = unlinked
            .then(|| meta_row("Agent", div().text_color(muted).child("Chat deleted"), muted));
        let updated = meta_row(
            "Updated",
            div().child(relative_time(card.updated_at, now, &chrono::Local)),
            muted,
        );
        let primary = match (card.chats.last().copied(), chat.map(|c| c.id)) {
            (None, _) => Some(
                aether_ui::button("board-run")
                    .primary()
                    .small()
                    .h(px(28.))
                    .icon(Lucide::Play)
                    .label("Run with agent")
                    .tooltip("Start an agent chat with this card as the prompt")
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.board_run(id, window, cx)),
                    )
                    .into_any_element(),
            ),
            (Some(_), Some(chat)) => Some(
                aether_ui::button("board-open-chat")
                    .outline()
                    .small()
                    .h(px(28.))
                    .icon(Lucide::MessageSquare)
                    .label("Open chat")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.board_open_chat(chat, window, cx)
                    }))
                    .into_any_element(),
            ),
            (Some(_), None) => None,
        };
        let delete = aether_ui::button("board-delete")
            .ghost()
            .small()
            .h(px(28.))
            .icon(Lucide::Trash)
            .label("Delete card")
            .on_click(cx.listener(move |this, _, window, cx| {
                this.confirm_delete_cards(vec![id], None, window, cx)
            }));
        Some(
            v_flex()
                .id("board-detail")
                .role(Role::Group)
                .aria_label("Card details")
                .flex_none()
                .w(px(340.))
                .min_h_0()
                .corner(px(12.))
                .border_1()
                .border_color(border)
                .bg(theme.background)
                .child(
                    h_flex()
                        .h(px(44.))
                        .pl(px(16.))
                        .pr(px(10.))
                        .gap(px(8.))
                        .items_center()
                        .border_b_1()
                        .border_color(border)
                        .child(col_chip)
                        .children(status)
                        .child(div().flex_1())
                        .child(close),
                )
                .child(
                    v_flex()
                        .id("board-detail-body")
                        .flex_1()
                        .min_h_0()
                        .p(px(16.))
                        .gap(px(12.))
                        .overflow_y_scroll()
                        .child(label("Title"))
                        .child(field(&self.board_title).aria_label("Card title"))
                        .child(label("Description"))
                        .child(field_area(&self.board_body).aria_label("Card description"))
                        .child(label("Labels"))
                        .children(chips)
                        .children(
                            labels_in
                                .map(|i| field(&i).small().aria_label("Labels, comma separated")),
                        )
                        .child(label("Blocked by"))
                        .children(blockers)
                        .children(blockers_in.map(|i| {
                            field(&i).small().aria_label("Blocking card ids, comma separated")
                        }))
                        .when(!agents.is_empty(), |d| d.child(label("Agents")).children(agents))
                        .child(label("Notes"))
                        .children(note_in.map(|i| field(&i).small().aria_label("Add a note")))
                        .children(notes)
                        .child(
                            v_flex()
                                .mt(px(4.))
                                .pt(px(10.))
                                .border_t_1()
                                .border_color(border)
                                .children(project)
                                .children(gone)
                                .child(updated),
                        ),
                )
                .child(
                    h_flex()
                        .p(px(12.))
                        .gap(px(8.))
                        .items_center()
                        .border_t_1()
                        .border_color(border)
                        .children(primary)
                        .child(div().flex_1())
                        .child(delete),
                ),
        )
    }
}
