use aether_ui::theme::Corners;
use std::path::Path;

use aether_core::home_dir;
use aether_ui::{
    ChoiceMenu, ChoiceOption, RecentRow, composer_chip, overlay, overlay_strong,
    project_mark_sized, provider_icon, theme,
};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable, checkbox::Checkbox, h_flex, v_flex,
};
use gpui_kit::{
    Anchor, AnyElement, App, Context, FontWeight, Hsla, Role, SharedString, div, img, prelude::*,
    px, transparent_black,
};

use super::model::{
    Activity, Card, CardStatus, Fact, Filter, GroupBy, ProjectData, SortBy, activity_label,
    activity_text, arrange, badge, card_aria, commit_activity, facts, filter_aria, in_filter,
    new_here_aria, recent, tree_badges, tree_label, verb,
};
use crate::app::{Bulk, ChatApp, now_ms};
use crate::open_in;
use crate::projects::relative_time;
use crate::workspace_popover::{choose_app_label, diff_stat, disabled_label, home_short};

const NO_FOLDER: &str = "Folder not found";

pub(crate) fn pill(text: String, bg: Hsla, fg: Hsla) -> impl IntoElement {
    div()
        .flex_none()
        .px(px(7.))
        .py(px(2.))
        .corner(px(6.))
        .text_xs()
        .bg(bg)
        .text_color(fg)
        .child(text)
}

fn caption(text: &'static str, cx: &Context<ChatApp>) -> impl IntoElement {
    div()
        .id(text)
        .role(Role::Heading)
        .mt(px(26.))
        .mb(px(8.))
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

pub(crate) fn status_tint(status: CardStatus, cx: &App) -> (Hsla, Hsla) {
    let theme = cx.theme();
    match status {
        CardStatus::Working => (theme.info.opacity(0.12), theme::info_text(cx)),
        CardStatus::Waiting(_) => (theme::mango().opacity(0.14), theme::mango_text(cx)),
        CardStatus::Failed => (theme.danger.opacity(0.12), theme::danger_text(cx)),
        CardStatus::Done | CardStatus::Idle => {
            (theme.success.opacity(0.12), theme::success_text(cx))
        }
    }
}

impl ChatApp {
    pub(crate) fn render_project_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(folder) = self.project_page.clone() else {
            return div().flex_1().into_any_element();
        };
        let now = now_ms();
        let cards = self.project_cards(&folder);
        let data = self.project_view.data.as_ref().filter(|d| d.folder == folder);
        let missing = self.missing_folders.contains(&folder);
        let header = self.project_header(&folder, data, missing, cx);
        let filters = self.project_filters(&cards, cx);
        let grid = self.project_grid(&cards, now, cx);
        let ids: Vec<i64> = cards
            .iter()
            .filter(|c| in_filter(self.project_view.filter, c.status))
            .map(|c| c.id)
            .collect();
        let picked: Vec<i64> =
            ids.iter().copied().filter(|id| self.project_picks.contains(id)).collect();
        let bulk = self
            .bulk_bar("project-bulk", picked, ids, &[Bulk::Archive, Bulk::Delete], true, cx)
            .map(|bar| div().mb(px(12.)).child(bar));
        let trees = data.filter(|d| !d.worktrees.is_empty()).map(|d| {
            v_flex().child(caption("Worktrees", cx)).child(
                v_flex()
                    .id("project-worktrees")
                    .role(Role::List)
                    .aria_label("Worktrees")
                    .children(self.project_trees(&folder, d, &cards, cx)),
            )
        });
        let chats = cards
            .iter()
            .map(|c| Activity::Chat {
                ix: c.ix,
                title: c.title.clone(),
                verb: verb(c.status),
                at_ms: c.activity_ms,
            })
            .collect();
        let commits = data.map(|d| commit_activity(&d.commits)).unwrap_or_default();
        let activity = recent(chats, commits, 8);
        let activity = (!activity.is_empty()).then(|| {
            v_flex().child(div().mx(px(8.)).child(caption("Recent activity", cx))).child(
                v_flex()
                    .id("project-activity")
                    .role(Role::List)
                    .aria_label("Recent activity")
                    .children(
                        activity
                            .into_iter()
                            .enumerate()
                            .map(|(i, a)| self.activity_row(i, a, &cards, now, cx)),
                    ),
            )
        });
        div()
            .id("project-page")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(980.))
                    .mx_auto()
                    .px(px(28.))
                    .pt(px(28.))
                    .pb(px(40.))
                    .child(header)
                    .child(filters)
                    .children(bulk)
                    .child(grid)
                    .children(trees)
                    .children(activity),
            )
            .into_any_element()
    }

    fn project_header(
        &self,
        folder: &Path,
        data: Option<&ProjectData>,
        missing: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let name = self.project_name(folder);
        let muted = cx.theme().muted_foreground;
        let status = data.and_then(|d| d.status.as_ref());
        let facts: Vec<_> = facts(home_short(folder, &home_dir()), missing, data)
            .into_iter()
            .map(|(kind, text)| {
                let icon = match kind {
                    Fact::Path => Lucide::Folder,
                    Fact::Branch => Lucide::GitBranch,
                    Fact::Changes => Lucide::FileDiff,
                    Fact::Remote => Lucide::Cloud,
                    Fact::Warn => Lucide::CircleAlert,
                };
                h_flex()
                    .gap(px(5.))
                    .items_center()
                    .min_w_0()
                    .child(Icon::new(icon).size(px(12.)))
                    .when(kind == Fact::Changes, |h| h.child(diff_stat(status, cx)))
                    .child(div().truncate().child(text))
            })
            .collect();
        let new_off = missing.then_some(NO_FOLDER);
        let new_folder = folder.to_path_buf();
        let new_agent = aether_ui::button("project-new-agent")
            .primary()
            .small()
            .h(px(28.))
            .icon(IconName::Plus)
            .label("New agent")
            .accessibility_label(
                new_off.map_or("New agent".into(), |r| disabled_label("New agent", r)),
            )
            .disabled(new_off.is_some())
            .when_some(new_off, |b, reason| b.tooltip(reason))
            .when(!missing, |b| {
                b.on_click(cx.listener(move |this, _, window, cx| {
                    this.new_chat_in(new_folder.clone(), window, cx)
                }))
            });
        h_flex()
            .gap(px(14.))
            .items_center()
            .child(project_mark_sized(&name, px(44.), cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .id("project-name")
                            .role(Role::Heading)
                            .truncate()
                            .text_size(px(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(name),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .mt(px(4.))
                            .gap_x(px(16.))
                            .gap_y(px(4.))
                            .text_xs()
                            .text_color(muted)
                            .children(facts),
                    ),
            )
            .child(self.project_open_in(folder, missing, cx))
            .child(new_agent)
    }

    fn project_open_in(
        &self,
        folder: &Path,
        missing: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let app_ix = open_in::project_app(&self.open_apps, &self.settings, folder);
        let app = app_ix.and_then(|ix| self.open_apps.get(ix));
        let options = self
            .open_apps
            .iter()
            .enumerate()
            .map(|(ix, a)| ChoiceOption {
                mark: None,
                icon: Some(a.glyph()),
                image: a.icon.clone(),
                title: a.name.into(),
                description: None,
                checked: Some(ix) == app_ix,
                disabled: false,
            })
            .collect();
        self.project_open_menu.update(cx, |m, cx| m.sync(options, None, 260., cx));
        let off =
            if missing { Some(NO_FOLDER) } else { app_ix.is_none().then_some("No apps found") };
        let label: SharedString =
            app.map_or("Open in…".into(), |a| format!("Open in {}", a.name).into());
        let mark = match app.and_then(|a| a.icon.clone()) {
            Some(path) => img(path).size(px(16.)).flex_none().into_any_element(),
            None => Icon::new(app.map_or(Lucide::Folder, |a| a.glyph())).small().into_any_element(),
        };
        let open_folder = folder.to_path_buf();
        let main = composer_chip("project-open-in", false, cx)
            .h(px(28.))
            .px(px(10.))
            .corner_l(px(6.))
            .corner_r(px(0.))
            .accessibility_label(off.map_or(label.to_string(), |r| disabled_label(&label, r)))
            .disabled(off.is_some())
            .when_some(off, |b, reason| b.tooltip(reason))
            .child(h_flex().gap(px(7.)).items_center().child(mark).child(label))
            .when_some(app_ix.filter(|_| !missing), |b, ix| {
                b.on_click(cx.listener(move |this, _, window, cx| {
                    this.open_project(open_folder.clone(), ix, window, cx)
                }))
            });
        let caret_off =
            missing.then_some(NO_FOLDER).or(self.open_apps.is_empty().then_some("No apps found"));
        let caret_label = choose_app_label(app.map(|a| a.name));
        let caret = composer_chip("project-open-in-menu", false, cx)
            .h(px(28.))
            .px(px(7.))
            .corner_r(px(6.))
            .corner_l(px(0.))
            .icon(IconName::ChevronDown)
            .accessibility_label(
                caret_off.map_or(caret_label.clone(), |r| disabled_label(&caret_label, r)),
            )
            .disabled(caret_off.is_some())
            .tooltip(caret_off.unwrap_or("Choose app"));
        h_flex()
            .flex_none()
            .h(px(30.))
            .items_center()
            .corner(px(6.))
            .border_1()
            .border_color(cx.theme().border)
            .child(main)
            .child(div().w(px(1.)).h(px(16.)).flex_none().bg(cx.theme().border))
            .child(ChoiceMenu::new(&self.project_open_menu, caret))
    }

    fn project_filters(&self, cards: &[Card], cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let view = &self.project_view;
        let theme = cx.theme();
        let (fg, muted, ring) = (theme.foreground, theme.muted_foreground, theme.ring);
        let (fill, on_fill) = (overlay(cx), overlay_strong(cx));
        let tabs = Filter::ALL.map(|f| {
            let n = cards.iter().filter(|c| in_filter(f, c.status)).count();
            let on = f == view.filter;
            div()
                .id(("project-filter", f as usize))
                .role(Role::Tab)
                .tab_index(0)
                .aria_label(filter_aria(f, n))
                .aria_selected(on)
                .flex()
                .gap(px(6.))
                .items_center()
                .h(px(26.))
                .px(px(10.))
                .corner(px(6.))
                .text_sm()
                .border_1()
                .border_color(transparent_black())
                .focus_visible(move |s| s.border_color(ring))
                .cursor_pointer()
                .text_color(if on { fg } else { muted })
                .when(on, |d| d.bg(on_fill))
                .hover(move |s| s.bg(fill))
                .active(move |s| s.bg(on_fill))
                .child(f.label())
                .child(div().text_color(muted).child(n.to_string()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.project_view.filter = f;
                    cx.notify();
                }))
        });
        let entity = cx.entity();
        let (group, sort) = (view.group, view.sort);
        let group_menu = {
            let entity = entity.clone();
            aether_ui::button("project-group")
                .ghost()
                .small()
                .label(format!("Group: {}", group.label()))
                .child(Icon::new(Lucide::ChevronDown).xsmall())
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu: PopupMenu, _, _| {
                    GroupBy::ALL.into_iter().fold(menu.label("Group by"), |menu, g| {
                        let entity = entity.clone();
                        menu.item(PopupMenuItem::new(g.label()).checked(g == group).on_click(
                            move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.project_view.group = g;
                                    cx.notify();
                                })
                            },
                        ))
                    })
                })
        };
        let sort_menu = aether_ui::button("project-sort")
            .ghost()
            .small()
            .label(format!("Sort: {}", sort.label()))
            .child(Icon::new(Lucide::ChevronDown).xsmall())
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu: PopupMenu, _, _| {
                SortBy::ALL.into_iter().fold(menu.label("Sort by"), |menu, s| {
                    let entity = entity.clone();
                    menu.item(PopupMenuItem::new(s.label()).checked(s == sort).on_click(
                        move |_, _, cx| {
                            entity.update(cx, |this, cx| {
                                this.project_view.sort = s;
                                cx.notify();
                            })
                        },
                    ))
                })
            });
        h_flex()
            .mt(px(22.))
            .mb(px(12.))
            .gap(px(4.))
            .items_center()
            .child(
                h_flex()
                    .id("project-filter-tabs")
                    .role(Role::TabList)
                    .aria_label("Filter agents")
                    .gap(px(4.))
                    .children(tabs),
            )
            .child(div().flex_1())
            .child(group_menu)
            .child(sort_menu)
    }

    fn project_grid(&self, cards: &[Card], now: i64, cx: &mut Context<Self>) -> AnyElement {
        let view = &self.project_view;
        let groups = arrange(cards.to_vec(), view.filter, view.group, view.sort);
        let theme = cx.theme();
        if groups.iter().all(|(_, list)| list.is_empty()) {
            return h_flex()
                .justify_center()
                .p(px(30.))
                .corner(px(12.))
                .border_1()
                .border_dashed()
                .border_color(theme.border)
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("No agents here.")
                .into_any_element();
        }
        let muted = theme.muted_foreground;
        let by_tree = view.group == GroupBy::Worktree;
        v_flex()
            .children(groups.into_iter().enumerate().map(|(i, (key, list))| {
                let cap = (!key.is_empty()).then(|| {
                    h_flex()
                        .gap(px(8.))
                        .items_center()
                        .when(i > 0, |h| h.mt(px(18.)))
                        .mb(px(8.))
                        .text_xs()
                        .text_color(muted)
                        .when(by_tree, |h| h.child(Icon::new(Lucide::GitBranch).size(px(13.))))
                        .child(
                            div()
                                .when(by_tree && list.iter().all(|c| c.gone), |d| d.line_through())
                                .child(key),
                        )
                        .child(list.len().to_string())
                });
                v_flex().children(cap).child(
                    h_flex()
                        .flex_wrap()
                        .gap(px(10.))
                        .children(list.iter().map(|card| {
                            div().flex_1().min_w(px(290.)).child(self.project_card(card, now, cx))
                        }))
                        .children((0..2).map(|_| div().flex_1().min_w(px(290.)).h_0())),
                )
            }))
            .into_any_element()
    }

    fn project_card(
        &self,
        card: &Card,
        now: i64,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let (muted, ring, fill, pressed) =
            (theme.muted_foreground, theme.ring, overlay(cx), overlay_strong(cx));
        let age = relative_time(card.activity_ms, now, &chrono::Local);
        let tint = status_tint(card.status, cx);
        let badge = badge(card.status, card.since_ms, now).map(|text| pill(text, tint.0, tint.1));
        let place = h_flex()
            .gap(px(10.))
            .items_center()
            .whitespace_nowrap()
            .text_xs()
            .text_color(muted)
            .child(
                h_flex()
                    .gap(px(4.))
                    .items_center()
                    .min_w_0()
                    .child(
                        Icon::new(if card.worktree { Lucide::GitFork } else { Lucide::GitBranch })
                            .size(px(12.)),
                    )
                    .child(
                        div()
                            .truncate()
                            .when(card.gone, |d| d.line_through())
                            .child(card.place.clone()),
                    ),
            )
            .when(card.added > 0, |m| {
                m.child(
                    div()
                        .flex_none()
                        .text_color(theme::success_text(cx))
                        .child(format!("+{}", card.added)),
                )
            })
            .when(card.removed > 0, |m| {
                m.child(
                    div()
                        .flex_none()
                        .text_color(theme::danger_text(cx))
                        .child(format!("\u{2212}{}", card.removed)),
                )
            })
            .when(card.ahead > 0, |m| m.child(aether_ui::arrow_count(Lucide::ArrowUp, card.ahead)));
        let thinking = (!card.effort.is_empty()).then(|| {
            let mut chars = card.effort.chars();
            let word: String =
                chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
            format!("{word} thinking")
        });
        let meta = h_flex()
            .gap(px(6.))
            .items_center()
            .whitespace_nowrap()
            .text_xs()
            .text_color(muted)
            .when(!card.model.is_empty(), |m| {
                m.child(div().min_w_0().truncate().child(card.model.clone()))
            })
            .when_some(thinking, |m, text| {
                m.child(div().flex_none().child("•")).child(div().flex_none().child(text))
            })
            .child(div().flex_1())
            .when(card.status != CardStatus::Working, |m| {
                m.child(div().flex_none().child(age.clone()))
            });
        let ix = card.ix;
        let id = card.id;
        let picked = self.project_picks.contains(&id);
        let picking = !self.project_picks.is_empty();
        let group = SharedString::from(format!("project-card-{ix}"));
        let icon_slot = div()
            .relative()
            .flex_none()
            .size(px(16.))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .when(picking, |d| d.opacity(0.))
                    .when(!picking, |d| d.group_hover(group.clone(), |s| s.opacity(0.)))
                    .child(provider_icon(card.provider).size(px(14.))),
            )
            .child(
                div()
                    .id(("project-card-pick-slot", ix))
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(!picking, |d| d.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.)))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        Checkbox::new(("project-card-pick", ix))
                            .checked(picked)
                            .tab_stop(false)
                            .aria_label(SharedString::from(format!("Select {}", card.title)))
                            .on_click(cx.listener(move |this, _: &bool, _, cx| {
                                if !this.project_picks.remove(&id) {
                                    this.project_picks.insert(id);
                                }
                                cx.notify();
                            })),
                    ),
            );
        v_flex()
            .id(("project-card", card.ix))
            .group(group)
            .role(Role::Button)
            .tab_index(0)
            .aria_label(card_aria(card, &age))
            .w_full()
            .h(px(138.))
            .overflow_hidden()
            .p(px(12.))
            .gap(px(8.))
            .corner(px(12.))
            .border_1()
            .border_color(if picked { ring } else { theme.border })
            .focus_visible(move |s| s.border_color(ring))
            .cursor_pointer()
            .hover(move |s| s.bg(fill))
            .active(move |s| s.bg(pressed))
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(icon_slot)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(card.title.clone()),
                    )
                    .children(badge),
            )
            .child(
                div()
                    .h(px(32.))
                    .text_xs()
                    .line_height(px(16.))
                    .text_color(muted)
                    .line_clamp(2)
                    .child(card.text.clone()),
            )
            .child(v_flex().mt_auto().gap(px(4.)).child(place).child(meta))
            .on_click(cx.listener(move |this, _, window, cx| this.select(ix, window, cx)))
    }

    fn project_trees(
        &self,
        folder: &Path,
        data: &ProjectData,
        cards: &[Card],
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let home = home_dir();
        let theme = cx.theme();
        let (muted, ring, border) = (theme.muted_foreground, theme.ring, theme.border);
        let (fill, pressed) = (overlay(cx), overlay_strong(cx));
        let tints = [
            (theme.muted, muted),
            (theme.info.opacity(0.12), theme::info_text(cx)),
            (theme::mango().opacity(0.14), theme::mango_text(cx)),
        ];
        data.worktrees
            .iter()
            .enumerate()
            .map(|(i, tree)| {
                let branch =
                    tree.branch.clone().unwrap_or_else(|| tree.head.chars().take(7).collect());
                let short = home_short(&tree.path, &home);
                let badges = tree_badges(cards, &data.worktrees, &tree.path);
                let label = tree_label(&branch, &short, tree.missing, &badges);
                let row_group = SharedString::from(format!("project-wt-{i}"));
                let (path, main, project) = (tree.path.clone(), tree.main, folder.to_path_buf());
                let plus = (!tree.missing).then(|| {
                    div()
                        .id(("project-wt-new", i))
                        .role(Role::Button)
                        .tab_index(0)
                        .aria_label(new_here_aria(&branch))
                        .flex()
                        .flex_none()
                        .size(px(24.))
                        .items_center()
                        .justify_center()
                        .corner(px(6.))
                        .border_1()
                        .border_color(transparent_black())
                        .opacity(0.)
                        .group_hover(row_group.clone(), |s| s.opacity(1.))
                        .focus_visible(move |s| s.border_color(ring).opacity(1.))
                        .cursor_pointer()
                        .hover(move |s| s.bg(fill))
                        .active(move |s| s.bg(pressed))
                        .tooltip(|window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new("New agent here")
                                .build(window, cx)
                        })
                        .child(Icon::new(Lucide::Plus).small())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.new_chat_in(project.clone(), window, cx);
                            if !main {
                                // open_worktree_chat arms the active chat's project; that's now this one
                                this.open_worktree_chat(path.clone(), false, window, cx);
                            }
                        }))
                });
                h_flex()
                    .id(("project-wt", i))
                    .group(row_group)
                    .role(Role::ListItem)
                    .aria_label(label)
                    .gap(px(10.))
                    .items_center()
                    .mb(px(6.))
                    .px(px(10.))
                    .py(px(6.))
                    .corner(px(10.))
                    .border_1()
                    .border_color(border)
                    .text_sm()
                    .child(
                        Icon::new(if tree.main { Lucide::House } else { Lucide::GitBranch })
                            .size(px(14.))
                            .text_color(muted),
                    )
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(6.))
                            .items_center()
                            .when(tree.missing, |h| h.line_through())
                            .child(div().min_w_0().truncate().child(branch))
                            .child(
                                div().min_w_0().truncate().text_xs().text_color(muted).child(short),
                            ),
                    )
                    .when(tree.missing, |r| {
                        r.child(div().flex_none().text_xs().text_color(muted).child("missing"))
                    })
                    .children(
                        badges
                            .into_iter()
                            .zip(tints)
                            .filter_map(|(text, (bg, fg))| text.map(|t| pill(t, bg, fg))),
                    )
                    .children(plus)
                    .into_any_element()
            })
            .collect()
    }

    fn activity_row(
        &self,
        i: usize,
        a: Activity,
        cards: &[Card],
        now: i64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let time = relative_time(a.at_ms(), now, &chrono::Local);
        let label = activity_label(&a, &time);
        let text = activity_text(&a);
        match a {
            Activity::Chat { ix, .. } => {
                let Some(card) = cards.iter().find(|card| card.ix == ix) else {
                    return div().into_any_element();
                };
                let view = cx.entity();
                RecentRow::new(("project-activity", i), provider_icon(card.provider), text, time)
                    .detail(Icon::new(Lucide::GitBranch), card.place.clone())
                    .accessibility_label(label)
                    .on_click(move |window, cx| {
                        view.update(cx, |this, cx| this.select(ix, window, cx));
                    })
                    .into_any_element()
            }
            Activity::Commit { short, subject, .. } => RecentRow::new(
                ("project-activity", i),
                Icon::new(Lucide::GitCommitHorizontal),
                subject,
                time,
            )
            .detail(Icon::new(Lucide::Hash), short)
            .accessibility_label(label)
            .into_any_element(),
        }
    }
}
