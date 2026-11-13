mod chart;
mod dialogs;
mod roster;
pub(crate) mod shared;
mod table;

use aether_core::OrgLayout;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::{AnyElement, Context, FontWeight, Role, Window, div, prelude::*, px};

use super::OrgOverlay;
use super::model::OrgSpec;
use crate::app::ChatApp;
use shared::avatar;

impl ChatApp {
    pub(crate) fn render_org(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let body = if !self.settings.org_enabled {
            self.org_off_state(cx).into_any_element()
        } else if self.org_current().is_none() {
            self.org_empty_state(window, cx).into_any_element()
        } else {
            match self.settings.org_layout {
                OrgLayout::Roster => self.render_org_roster(window, cx),
                OrgLayout::Chart => self.render_org_chart(window, cx),
                OrgLayout::Table => self.render_org_table(window, cx),
            }
        };
        let lights = aether_ui::traffic_lights_gap(self.settings.sidebar_collapsed, window, cx);
        let overlay = self.render_org_overlay(window, cx);
        v_flex()
            .size_full()
            .relative()
            .bg(cx.theme().background)
            .child(h_flex().child(lights))
            .child(body)
            .children(overlay)
            .into_any_element()
    }

    fn render_org_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        match self.org.overlay.clone() {
            OrgOverlay::None => None,
            OrgOverlay::Wizard { step, draft } => self.render_org_wizard(step, &draft, window, cx),
            OrgOverlay::HeadEditor(role) => self.render_org_head_editor(&role, window, cx),
            OrgOverlay::Hire(draft) => self.render_org_hire(&draft, window, cx),
        }
    }

    fn org_off_state(&self, cx: &mut Context<Self>) -> impl IntoElement {
        empty_box(
            "Organizations are turned off",
            "Run a project like a small company: a lead that routes work, persistent heads with their own models, and interns that scale with your machine. Off by default because heads keep spending tokens while they work.",
            aether_ui::button("org-turn-on").primary().small().label("Turn on").on_click(
                cx.listener(|this, _, _, cx| this.update_settings(cx, |s| s.org_enabled = true)),
            ),
            cx,
        )
    }

    fn org_empty_state(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().flex_1().min_h_0().child(self.org_header(None, window, cx)).child(empty_box(
            "No organization in this project yet",
            "Pick a template and Aether creates the head chats, pins them, turns on their lead tools, writes their role files and wires up review. You can change everyone later.",
            aether_ui::button("org-setup").primary().small().label("Set up organization").on_click(
                cx.listener(|this, _, _, cx| {
                    let s = &this.settings;
                    let draft = OrgSpec::from_template(&s.org_template, s.org_workers_per_head);
                    this.org.overlay = OrgOverlay::Wizard { step: 0, draft };
                    cx.notify();
                }),
            ),
            cx,
        ))
    }
}

fn empty_box(
    title: &'static str,
    text: &'static str,
    action: impl IntoElement,
    cx: &Context<ChatApp>,
) -> impl IntoElement {
    let theme = cx.theme();
    div().flex_1().flex().items_center().justify_center().child(
        v_flex()
            .max_w(px(460.))
            .gap(px(12.))
            .items_center()
            .child(avatar("CEO", 40.))
            .child(
                div()
                    .id(title)
                    .role(Role::Heading)
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_center()
                    .text_color(theme.muted_foreground)
                    .child(text),
            )
            .child(action),
    )
}
