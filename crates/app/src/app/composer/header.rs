use crate::session_workspace::WindowId;
use aether_core::home_dir;
use aether_ui::theme::Corners;
use std::path::{Path, PathBuf};

use aether_core::Provider;
use aether_ui::{
    anchored_popup, icon_chip, overlay, project_color, project_mark, provider_icon, theme,
};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::{ActiveTheme, IconName, Selectable, h_flex};
use gpui_kit::{
    Anchor, AnyElement, Context, MouseButton, Role, SharedString, Window, div, prelude::*, px,
    transparent_black,
};

use crate::actions::ToggleSidebar;
use crate::app::sidebar::folder_label;
use crate::app::start::{version_chip, version_text};
use crate::app::{ChatApp, project_dir};
use crate::open_in;

impl ChatApp {
    pub(in crate::app) fn load_open_apps(&mut self, cx: &mut Context<Self>) {
        if !cfg!(target_os = "macos") {
            return;
        }
        let cache = open_in::icon_cache();
        let found = cx.background_executor().spawn(async move {
            open_in::with_terminal(open_in::discover(&open_in::KNOWN, &home_dir()))
        });
        cx.spawn(async move |this, cx| {
            let apps = found.await;
            let Ok(list) = this.update(cx, |this, cx| {
                this.open_apps = apps.clone();
                cx.notify();
                apps
            }) else {
                return;
            };
            let icons =
                cx.background_executor().spawn(async move { open_in::icons(&list, &cache) }).await;
            this.update(cx, |this, cx| {
                for (app, icon) in this.open_apps.iter_mut().zip(icons) {
                    app.icon = icon;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn open_project(
        &mut self,
        folder: PathBuf,
        app_ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(app) = self.open_apps.get(app_ix) else {
            return;
        };
        if app.id == open_in::AETHER_TERMINAL_ID {
            return self.open_in_terminal(folder, window, cx);
        }
        let bundle = app.bundle.clone();
        self.open_error = None;
        let task = cx.background_executor().spawn(async move { open_in::open(&bundle, &folder) });
        cx.spawn(async move |this, cx| {
            if let Err(e) = task.await {
                this.update(cx, |this, cx| {
                    this.open_error = Some(format!("Couldn't open: {e}").into());
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn project_name(&self, dir: &Path) -> SharedString {
        self.project_names()
            .remove(dir)
            .unwrap_or_else(|| folder_label(&dir.display().to_string()))
            .into()
    }

    pub(crate) fn chat_project(&self, ix: usize) -> Option<PathBuf> {
        let dir = project_dir(&self.chats[ix], &self.settings.workspace_root);
        (dir != home_dir()).then_some(dir)
    }

    pub(crate) fn project_crumb(
        &self,
        name: &SharedString,
        dir: PathBuf,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (ring, fill) = (cx.theme().ring, overlay(cx));
        h_flex()
            .id("crumb-project")
            .on_mouse_down(MouseButton::Right, crate::session_workspace::block_session_menu)
            .role(Role::Link)
            .tab_index(0)
            .aria_label(project_page_aria(name))
            .flex_none()
            .gap(px(6.))
            .items_center()
            .min_h(px(24.))
            .px(px(4.))
            .corner(px(6.))
            .border_1()
            .border_color(transparent_black())
            .focus_visible(move |s| s.border_color(ring))
            .cursor_pointer()
            .hover(move |s| s.bg(fill))
            .active(move |s| s.bg(fill))
            .child(project_mark(Some(name), cx))
            .child(div().text_sm().text_color(project_color(name, cx)).child(name.clone()))
            .on_click(self.session_listener(cx, move |this, _, _, cx| {
                this.open_project_page(dir.clone(), cx)
            }))
    }

    fn open_error_line(&self, cx: &Context<Self>) -> Option<impl IntoElement + use<>> {
        let e = self.open_error.clone()?;
        let tip = e.clone();
        Some(
            div()
                .id("open-error")
                .min_w_0()
                .truncate()
                .text_xs()
                .text_color(theme::danger_text(cx))
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                })
                .child(e),
        )
    }

    pub(in crate::app) fn render_project_top_bar(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let expand_btn =
            self.settings.sidebar_collapsed.then(|| {
                aether_ui::button("expand-sidebar")
                    .on_mouse_down(MouseButton::Right, crate::session_workspace::block_session_menu)
                    .ghost()
                    .small()
                    .icon(IconName::PanelLeftOpen)
                    .accessibility_label("Expand sidebar")
                    .tooltip_with_action("Expand sidebar", &ToggleSidebar, None)
                    .on_click(self.session_listener(cx, |this, _, window, cx| {
                        this.toggle_sidebar(window, cx)
                    }))
            });
        let name = self.project_page.as_deref().map(|dir| self.project_name(dir));
        h_flex()
            .flex_none()
            .h(px(40.))
            .px(px(16.))
            .gap(px(8.))
            .items_center()
            .children(expand_btn)
            .children(name.map(|name| {
                h_flex()
                    .id("breadcrumb")
                    .role(Role::Group)
                    .aria_label(name.clone())
                    .gap(px(6.))
                    .items_center()
                    .min_w_0()
                    .child(project_mark(Some(&name), cx))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .text_color(project_color(&name, cx))
                            .child(name),
                    )
            }))
            .child(div().flex_1())
            .children(self.open_error_line(cx))
    }

    pub(in crate::app) fn render_header(
        &self,
        pop: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        self.render_header_content(pop, None, true, true, 0, cx)
    }

    pub(crate) fn render_header_with_pane(
        &self,
        pop: Option<AnyElement>,
        controls: AnyElement,
        show_sidebar: bool,
        show_tools: bool,
        host: WindowId,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_header_content(pop, Some(controls), show_sidebar, show_tools, host, cx)
            .into_any_element()
    }

    pub(crate) fn delegated_by(&self, ix: usize) -> Option<usize> {
        let parent = self.chats[ix].parent?;
        self.chats.iter().position(|c| c.id == parent && c.lead)
    }

    fn render_header_content(
        &self,
        pop: Option<AnyElement>,
        pane_controls: Option<AnyElement>,
        show_sidebar: bool,
        show_tools: bool,
        host: WindowId,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let compact = pane_controls.is_some();
        let chat = &self.chats[self.active];
        let expand_btn =
            (show_sidebar && self.settings.sidebar_collapsed).then(|| {
                aether_ui::button("expand-sidebar")
                    .on_mouse_down(MouseButton::Right, crate::session_workspace::block_session_menu)
                    .ghost()
                    .small()
                    .icon(IconName::PanelLeftOpen)
                    .accessibility_label("Expand sidebar")
                    .tooltip_with_action("Expand sidebar", &ToggleSidebar, None)
                    .on_click(self.session_listener(cx, |this, _, window, cx| {
                        this.toggle_sidebar(window, cx)
                    }))
            });
        let is_agent = self.is_agent_chat(chat);
        let back_btn = (!is_agent).then(|| {
            aether_ui::button("chat-header-back")
                .on_mouse_down(MouseButton::Right, crate::session_workspace::block_session_menu)
                .ghost()
                .small()
                .icon(IconName::ArrowLeft)
                .accessibility_label("Back to Agent Projects")
                .tooltip("Back to Agent Projects")
                .on_click(self.session_listener(cx, |this, _, window, cx| {
                    this.switch_sidebar_to_projects(window, cx);
                }))
        });
        let project: Option<SharedString> =
            self.chat_project(self.active).filter(|_| is_agent).map(|dir| self.project_name(&dir));
        let aria = breadcrumb_aria(project.as_deref(), &chat.title);
        let muted = cx.theme().muted_foreground;
        let crumb = project
            .as_ref()
            .zip(self.chat_project(self.active))
            .map(|(name, dir)| self.project_crumb(name, dir, cx));
        let delegated = self.delegated_by(self.active).map(|lead_ix| {
            let lead = &self.chats[lead_ix];
            let (lead_id, ring, fill) = (lead.id, cx.theme().ring, overlay(cx));
            div()
                .id("delegated-by")
                .debug_selector(|| "delegated-by".into())
                .on_mouse_down(MouseButton::Right, crate::session_workspace::block_session_menu)
                .role(Role::Link)
                .tab_index(0)
                .aria_label(delegated_aria(&lead.title))
                .min_w_0()
                .truncate()
                .px(px(4.))
                .corner(px(6.))
                .border_1()
                .border_color(transparent_black())
                .focus_visible(move |s| s.border_color(ring))
                .hover(move |s| s.bg(fill))
                .active(move |s| s.bg(fill))
                .text_sm()
                .text_color(muted)
                .cursor_pointer()
                .child(format!("Delegated by {}", lead.title))
                .on_click(self.session_listener(cx, move |this, _, window, cx| {
                    this.open_chat(lead_id, window, cx)
                }))
        });
        let handoff = chat.handoff_from.and_then(|id| self.chat_ix(id)).map(|ix| {
            let source = &self.chats[ix];
            let (source_id, ring, fill) = (source.id, cx.theme().ring, overlay(cx));
            div()
                .id("handoff-from")
                .on_mouse_down(MouseButton::Right, crate::session_workspace::block_session_menu)
                .role(Role::Link)
                .tab_index(0)
                .aria_label(format!("Open source chat {}", source.title))
                .min_w_0()
                .truncate()
                .px(px(4.))
                .corner(px(6.))
                .border_1()
                .border_color(transparent_black())
                .focus_visible(move |s| s.border_color(ring))
                .hover(move |s| s.bg(fill))
                .active(move |s| s.bg(fill))
                .text_sm()
                .text_color(muted)
                .cursor_pointer()
                .child(format!("from {}", source.title))
                .on_click(self.session_listener(cx, move |this, _, window, cx| {
                    if let Some(ix) = this.chat_ix(source_id) {
                        this.select(ix, window, cx)
                    }
                }))
        });
        let workspace_btn = (is_agent && show_tools).then(|| {
            let open = pop.is_some();
            let toggle = icon_chip("workspace-toggle", open, cx)
                .selected(open)
                .toggled(open)
                .icon(Lucide::LayoutList)
                .accessibility_label("Workspace")
                .tooltip("Workspace")
                .when(!open, |d| {
                    // open only: outside mouse-down already closed it, a click would reopen
                    d.on_click(self.session_listener(cx, |this, _, window, cx| {
                        this.toggle_workspace_pop(window, cx)
                    }))
                });
            anchored_popup("workspace-pop-anchor", toggle, &self.workspace_pop.trigger, true)
                .anchor(Anchor::TopRight)
                .when_some(pop, |p, pop| p.content(pop))
        });

        h_flex()
            .flex_none()
            .h(px(if compact { 32. } else { 40. }))
            .w_full()
            .min_w_0()
            .px(px(if compact { 12. } else { 16. }))
            .gap(px(8.))
            .items_center()
            .justify_between()
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    .children(back_btn)
                    .children(expand_btn)
                    .child(
                        h_flex()
                            .id("breadcrumb")
                            .role(Role::Group)
                            .aria_label(aria)
                            .gap(px(6.))
                            .items_center()
                            .min_w_0()
                            .when(compact, |h| {
                                h.child(self.provider_version_chip(chat.provider, true, cx))
                            })
                            .children(crumb)
                            .when(project.is_some(), |h| {
                                h.child(div().flex_none().text_sm().text_color(muted).child("/"))
                            })
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .text_color(if is_agent {
                                        cx.theme().foreground
                                    } else {
                                        muted
                                    })
                                    .child(chat.title.clone()),
                            )
                            .when(delegated.is_some(), |h| {
                                h.child(div().flex_none().text_sm().text_color(muted).child("·"))
                            })
                            .children(delegated)
                            .when(handoff.is_some(), |h| {
                                h.child(div().flex_none().text_sm().text_color(muted).child("·"))
                            })
                            .children(handoff),
                    ),
            )
            .child(
                h_flex()
                    .id("session-header-tools")
                    .on_mouse_down(MouseButton::Right, crate::session_workspace::block_session_menu)
                    .gap(px(8.))
                    .items_center()
                    .flex_none()
                    .when(show_tools, |h| h.children(self.open_error_line(cx)))
                    .when(is_agent && show_tools, |h| {
                        h.when(!compact, |h| {
                            h.child(self.provider_version_chip(chat.provider, false, cx))
                        })
                        .children(self.background_toggle(cx))
                        .children(workspace_btn)
                        .child(self.side_button(host, cx))
                    })
                    .when(!is_agent && show_tools, |h| {
                        h.child(
                            div()
                                .px_2()
                                .py_0p5()
                                .corner(px(4.))
                                .text_xs()
                                .bg(cx.theme().muted.opacity(0.5))
                                .text_color(cx.theme().muted_foreground)
                                .child("Direct Chat"),
                        )
                    })
                    .children(pane_controls),
            )
    }
}

impl ChatApp {
    fn provider_version_chip(
        &self,
        provider: Provider,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let text =
            version_chip(provider.label(), &version_text(provider, self.probes.get(&provider)));
        let tip = text.clone();
        h_flex()
            .id("provider-version")
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
            })
            .gap(px(4.))
            .items_center()
            .when(!compact, |h| h.px_2().py_0p5().corner(px(6.)).bg(cx.theme().muted.opacity(0.5)))
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(provider_icon(provider).size(px(12.)))
            .when(!compact, |h| h.child(text))
    }
}

pub(crate) fn project_page_aria(project: &str) -> String {
    format!("Open project page for {project}")
}

pub(crate) fn delegated_aria(lead: &str) -> String {
    format!("Open lead {lead}")
}

pub(crate) fn breadcrumb_aria(project: Option<&str>, title: &str) -> String {
    match project {
        Some(p) => format!("{p} / {title}"),
        None => title.to_string(),
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/composer/header.rs"]
mod tests;
