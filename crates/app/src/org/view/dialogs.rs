use std::rc::Rc;

use aether_core::{OrgHire, Provider};
use aether_ui::{AgentSelector, AgentSelectorEvent, AgentSelectorState, field, field_area};
use gpui_kit::base::{FocusTrapElement, actions::Cancel};
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Context, Div, ElementId, Entity, FocusHandle, FontWeight, Hsla, MouseButton,
    Role, Window, div, prelude::*, px, rgb,
};

use crate::app::{CLIS, ChatApp, Page, stored_model};
use crate::board::model::Col;
use crate::org::model::{
    Head, HireSpec, OrgSpec, ROLES, TEMPLATES, hire_checks, memory_path, models, role_file_text,
    role_path,
};
use crate::org::view::shared::{avatar, report};
use crate::org::{OrgOverlay, org_dir};
use crate::project_page::view::pill;
use crate::settings_view::SettingsTab;

const STEPS: [&str; 4] = ["Template", "Staff", "Rules", "Review"];
const GREEN: u32 = 0x3fb950;
const RED: u32 = 0xf85149;
const BLUE: u32 = 0x58a6ff;

type Pick<T> = Rc<dyn Fn(&mut ChatApp, T, &mut Window, &mut Context<ChatApp>)>;
type StaffInputs = Vec<(String, Entity<InputState>)>;
/// Edits a provider, model and effort held somewhere in the app.
type Agent<'a> = &'a mut dyn FnMut(&mut Provider, &mut String, &mut String);

pub(crate) fn parse_count(text: &str) -> Option<usize> {
    text.trim().parse().ok()
}

/// Worker models of `providers`, cheapest tier of each first.
pub(crate) fn hire_models(providers: &[Provider]) -> Vec<(Provider, &'static str)> {
    let mut out: Vec<(usize, Provider, &'static str)> = providers
        .iter()
        .flat_map(|&p| models(p).iter().rev().enumerate().map(move |(tier, m)| (tier, p, *m)))
        .collect();
    out.sort_by_key(|o| o.0);
    out.into_iter().map(|(_, p, m)| (p, m)).collect()
}

/// Wizard Rules step; applied to settings only on Create.
struct RulesForm {
    manual: bool,
    hire: OrgHire,
    pool: Entity<InputState>,
    builds: Entity<InputState>,
    per_head: Entity<InputState>,
    rotate: Entity<InputState>,
    goal: Entity<InputState>,
}

struct HeadForm {
    head: Head,
    title: Entity<InputState>,
    owns: Entity<InputState>,
    workers: Entity<InputState>,
    role_text: Entity<TextareaState>,
    memory: String,
}

fn new_input(value: String, window: &mut Window, cx: &mut App) -> Entity<InputState> {
    cx.new(|cx| {
        let mut s = InputState::new(window, cx);
        s.set_value(value, window, cx);
        s
    })
}

fn keyed_input(id: String, value: String, window: &mut Window, cx: &mut App) -> Entity<InputState> {
    window.use_keyed_state(ElementId::Name(id.into()), cx, move |window, cx| {
        let mut s = InputState::new(window, cx);
        s.set_value(value, window, cx);
        s
    })
}

fn value(input: &Entity<InputState>, cx: &App) -> String {
    input.read(cx).value().trim().to_string()
}

fn set(input: &Entity<InputState>, text: String, window: &mut Window, cx: &mut App) {
    input.update(cx, |s, cx| s.set_value(text, window, cx));
}

/// Dropdown of `options`; picking one runs `pick`.
fn select<T: Clone + PartialEq + 'static>(
    id: impl Into<ElementId>,
    current: T,
    options: Vec<(T, String)>,
    cx: &mut Context<ChatApp>,
    pick: impl Fn(&mut ChatApp, T, &mut Window, &mut Context<ChatApp>) + 'static,
) -> AnyElement {
    let label = options
        .iter()
        .find(|o| o.0 == current)
        .or(options.first())
        .map(|o| o.1.clone())
        .unwrap_or_default();
    let app = cx.entity();
    let pick: Pick<T> = Rc::new(pick);
    aether_ui::button(id)
        .small()
        .w_full()
        .label(label)
        .dropdown_caret(true)
        .dropdown_menu(move |mut menu, _, _| {
            for (value, text) in options.clone() {
                let (app, pick) = (app.clone(), pick.clone());
                let checked = value == current;
                menu = menu.item(PopupMenuItem::new(text).checked(checked).on_click(
                    move |_, window, cx| {
                        app.update(cx, |this, cx| {
                            pick(this, value.clone(), window, cx);
                            cx.notify();
                        })
                    },
                ));
            }
            menu
        })
        .into_any_element()
}

fn labeled(label: impl IntoElement, control: impl IntoElement, muted: Hsla) -> Div {
    v_flex()
        .flex_1()
        .min_w_0()
        .gap(px(5.))
        .child(div().text_size(px(12.)).text_color(muted).child(label))
        .child(control)
}

fn help(text: &'static str, muted: Hsla) -> impl IntoElement {
    div().text_size(px(11.5)).text_color(muted).child(text)
}

fn row2(a: impl IntoElement, b: impl IntoElement) -> Div {
    h_flex().gap(px(12.)).items_start().child(a).child(b)
}

fn staff_row(cells: [AnyElement; 5], border: Hsla) -> Div {
    let widths = [Some(22.), None, Some(260.), Some(120.), Some(28.)];
    h_flex().gap(px(8.)).py(px(7.)).items_center().border_b_1().border_color(border).children(
        cells.into_iter().zip(widths).map(|(cell, w)| match w {
            Some(w) => div().w(px(w)).flex_none().child(cell),
            None => div().flex_1().min_w_0().child(cell),
        }),
    )
}

fn check_pill(text: String, ok: bool) -> impl IntoElement {
    let color: Hsla = rgb(if ok { GREEN } else { RED }).into();
    pill(format!("{} {text}", if ok { "✓" } else { "✗" }), color.opacity(0.15), color)
}

fn heading(id: &'static str, text: impl Into<gpui_kit::SharedString>) -> impl IntoElement {
    div()
        .id(id)
        .role(Role::Heading)
        .text_size(px(15.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.into())
}

/// Lives exactly as long as the modal is rendered. Release also covers success paths
/// which dismiss the overlay in the org model or navigate to another page.
struct OrgDialogFocus {
    focus: FocusHandle,
    initial: FocusHandle,
    invoking: Option<FocusHandle>,
}

fn dialog_focus(window: &mut Window, cx: &mut App) -> Entity<OrgDialogFocus> {
    window.use_keyed_state("org-dialog-focus", cx, |window, cx| {
        let invoking = window.focused(cx);
        let focus = cx.focus_handle();
        let initial = cx.focus_handle();
        initial.focus(window, cx);
        // Wizard steps and staff removal can unmount the focused control.
        // Keep the app's general focus-lost fallback from returning to the page.
        cx.on_focus_lost(window, |state: &mut OrgDialogFocus, window, cx| {
            let target = window.focus_lost_restore_target(cx);
            if target.as_ref() == Some(&state.initial) || target.as_ref() == Some(&state.focus) {
                state.initial.focus(window, cx);
            }
        })
        .detach();
        cx.on_release_in(window, |state: &mut OrgDialogFocus, window, cx| {
            // Navigation may already have focused a destination control.
            let restore = window.focused(cx).is_none()
                || state.focus.contains_focused(window, cx)
                || state.initial.is_focused(window);
            if restore && let Some(invoking) = state.invoking.take() {
                invoking.focus(window, cx);
            }
        })
        .detach();
        OrgDialogFocus { focus, initial, invoking }
    })
}

fn dialog_panel(panel: Div, name: String, initial: &FocusHandle) -> gpui_kit::Stateful<Div> {
    panel
        .id("org-dialog")
        .role(Role::Dialog)
        .aria_label(name)
        .a11y_synthetic_children(|tree| tree.parent_node().set_modal())
        .track_focus(initial)
}

/// Use the native dialog action, rather than a raw key listener: nested menus
/// and AgentSelector consume their own Escape binding before it reaches us.
fn dialog_surface(
    panel: Div,
    name: String,
    focus: &FocusHandle,
    initial: &FocusHandle,
    drawer: bool,
    close: impl Fn(&Cancel, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let panel_focus = initial.clone();
    // Keep the semantic node inside the trap: FocusTrapContainer does not
    // forward Element's accessibility hooks to its wrapped div in GPUI Base.
    div()
        .flex()
        .when(drawer, |d| d.h_full())
        .child(dialog_panel(panel, name, initial).key_context("Dialog").on_action(
            move |action: &Cancel, window, cx| {
                // Native input and popup handlers get first chance. An unhandled
                // field Escape exits editing before a deliberate panel dismissal.
                if window.has_focused_input(cx) {
                    panel_focus.focus(window, cx);
                } else {
                    close(action, window, cx);
                }
            },
        ))
        .focus_trap("org-dialog-trap", focus)
}

/// Shared by org overlays and their native interaction regressions.
fn dialog_scrim(
    drawer: bool,
    panel: Div,
    name: String,
    window: &mut Window,
    cx: &mut App,
    close: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let state = dialog_focus(window, cx);
    let focus = state.read(cx).focus.clone();
    let initial = state.read(cx).initial.clone();
    let close: Rc<dyn Fn(&mut Window, &mut App)> = Rc::new(close);
    let backdrop_close = close.clone();
    div()
        .id("org-scrim")
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .bg(gpui_kit::black().opacity(0.55))
        .map(|d| if drawer { d.justify_end() } else { d.items_center().justify_center() })
        .on_mouse_down(MouseButton::Left, move |_, window, cx| backdrop_close(window, cx))
        .child(dialog_surface(
            panel
                .occlude()
                .flex()
                .flex_col()
                .overflow_hidden()
                .bg(cx.theme().background)
                .border_1()
                .border_color(cx.theme().border)
                .text_size(px(13.)),
            name,
            &focus,
            &initial,
            drawer,
            move |_: &Cancel, window, cx| close(window, cx),
        ))
        .into_any_element()
}

/// A non-modal picker closes when Tab leaves it. Otherwise its deferred
/// popup stays open while Escape is routed to the surrounding modal instead.
fn dialog_agent_selector(
    state: &Entity<AgentSelectorState>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let selector = state.downgrade();
    let marker = format!("OrgAgentSelector{}", state.entity_id().as_u64());
    let observed_marker = marker.clone();
    let tab_selector = selector.clone();
    let boundary =
        window.use_keyed_state(("org-agent-boundary", state.entity_id()), cx, |window, cx| {
            let focus = cx.focus_handle();
            // Tab may return to the trigger, which is still inside this boundary.
            // Observe after native key actions, without intercepting rail/effort navigation.
            cx.observe_keystrokes(move |_, event, window, cx| {
                let contains = |contexts: &[gpui_kit::KeyContext], name: &str| {
                    contexts.iter().any(|context| context.contains(name))
                };
                if event.keystroke.key == "tab"
                    && contains(&event.context_stack, &observed_marker)
                    && contains(&event.context_stack, aether_ui::AGENT_SELECTOR_CONTEXT)
                    && !contains(&window.context_stack(), aether_ui::AGENT_SELECTOR_CONTEXT)
                {
                    tab_selector.update(cx, |state, cx| state.dismiss(cx)).ok();
                }
            })
            .detach();
            cx.on_focus_out(&focus, window, move |_, _, _, cx| {
                selector.update(cx, |state, cx| state.dismiss(cx)).ok();
            })
            .detach();
            focus
        });
    div()
        .key_context(marker.as_str())
        .track_focus(boundary.read(cx))
        .child(AgentSelector::new(state))
        .into_any_element()
}

impl ChatApp {
    fn org_close(&mut self, cx: &mut Context<Self>) {
        self.org.overlay = OrgOverlay::None;
        cx.notify();
    }

    /// Dimmed backdrop; a click on it closes the overlay. `drawer` docks the panel right.
    fn org_scrim(
        &self,
        drawer: bool,
        panel: Div,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let app = cx.entity().downgrade();
        dialog_scrim(drawer, panel, name, window, cx, move |_, cx| {
            app.update(cx, |this, cx| this.org_close(cx)).ok();
        })
    }

    /// The composer's agent picker over the lead CLIs; its Reasoning row offers the model's efforts.
    #[allow(clippy::too_many_arguments)]
    fn agent_picker(
        &self,
        key: String,
        provider: Provider,
        model: &str,
        effort: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        edit: impl Fn(&mut ChatApp, Agent<'_>, &mut Context<ChatApp>) + 'static,
    ) -> AnyElement {
        let app = cx.entity().downgrade();
        let state = window.use_keyed_state(ElementId::Name(key.into()), cx, |window, cx| {
            cx.subscribe_self(move |_, ev: &AgentSelectorEvent, cx| {
                app.update(cx, |this, cx| {
                    match ev {
                        AgentSelectorEvent::Pick { provider, model } => {
                            let model = stored_model(*provider, model);
                            let efforts = this.model_efforts(*provider, &model);
                            let pick = &mut |p: &mut Provider, m: &mut String, e: &mut String| {
                                (*p, *m) = (*provider, model.clone());
                                if !efforts.contains(e) {
                                    e.clear();
                                }
                            };
                            edit(this, pick, cx)
                        }
                        AgentSelectorEvent::Effort(level) => {
                            edit(this, &mut |_, _, e| *e = level.clone(), cx)
                        }
                        AgentSelectorEvent::ToggleFavorite { provider, model } => {
                            this.toggle_favorite(*provider, model, cx)
                        }
                        AgentSelectorEvent::OpenSettings => {
                            this.org.overlay = OrgOverlay::None;
                            this.settings_tab = SettingsTab::Providers;
                            this.open_page(Page::Settings, cx);
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
            AgentSelectorState::new(window, cx)
        });
        let (providers, picked) = self.selector_rows(&CLIS, provider, model);
        let efforts = self.model_efforts(provider, model);
        let (favorites, effort) = (self.settings.favorite_models.clone(), effort.to_string());
        state.update(cx, |s, cx| {
            s.sync(providers, favorites, provider, picked, efforts, effort, cx)
        });
        dialog_agent_selector(&state, window, cx)
    }

    fn org_wizard_draft(&mut self) -> Option<&mut OrgSpec> {
        match &mut self.org.overlay {
            OrgOverlay::Wizard { draft, .. } => Some(draft),
            _ => None,
        }
    }

    fn org_wizard_head(&mut self, role: &str, f: impl FnOnce(&mut Head)) {
        if let Some(h) =
            self.org_wizard_draft().and_then(|d| d.heads.iter_mut().find(|h| h.role == role))
        {
            f(h);
        }
    }

    /// Writes the Staff inputs into the draft, then moves to `step`.
    fn org_wizard_go(&mut self, step: usize, staff: &StaffInputs, cx: &mut Context<Self>) {
        if let OrgOverlay::Wizard { step: at, draft } = &mut self.org.overlay {
            for (role, title) in staff {
                let Some(h) = draft.heads.iter_mut().find(|h| &h.role == role) else { continue };
                let title = value(title, cx);
                if !title.is_empty() {
                    h.title = title;
                }
            }
            *at = step;
        }
        cx.notify();
    }

    fn org_wizard_create(
        &mut self,
        rules: &Entity<RulesForm>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut spec) = self.org_wizard_draft().cloned() else { return };
        let f = rules.read(cx);
        let pool = if f.manual { parse_count(&value(&f.pool, cx)).map(|n| n.max(1)) } else { None };
        let builds = parse_count(&value(&f.builds, cx));
        let per_head = parse_count(&value(&f.per_head, cx));
        let rotate = parse_count(&value(&f.rotate, cx)).map(|n| n.clamp(10, 100) as u8);
        let hire = f.hire;
        spec.goal = value(&f.goal, cx);
        self.update_settings(cx, |s| {
            s.org_pool = pool;
            s.org_hire = hire;
            s.org_builds = builds.unwrap_or(s.org_builds).max(1);
            s.org_workers_per_head = per_head.unwrap_or(s.org_workers_per_head);
            s.org_rotate_at = rotate.unwrap_or(s.org_rotate_at);
        });
        for h in &mut spec.heads {
            h.max_workers = self.settings.org_workers_per_head;
        }
        match self.org_create(spec, window, cx) {
            Ok(()) => self.org_close(cx),
            Err(e) => report::<()>(Err(e), window, cx),
        }
    }

    /// Setup wizard overlay: Template, Staff, Rules, Review; Create calls `org_create`.
    pub(crate) fn render_org_wizard(
        &mut self,
        step: usize,
        draft: &OrgSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let s = &self.settings;
        let init = (s.org_pool, s.org_builds, s.org_workers_per_head, s.org_hire, s.org_rotate_at);
        let rules = window.use_keyed_state("org-wizard-rules", cx, move |window, cx| {
            let (pool, builds, per_head, hire, rotate) = init;
            RulesForm {
                manual: pool.is_some(),
                hire,
                pool: new_input(pool.unwrap_or(3).to_string(), window, cx),
                builds: new_input(builds.to_string(), window, cx),
                per_head: new_input(per_head.to_string(), window, cx),
                rotate: new_input(rotate.to_string(), window, cx),
                goal: cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("e.g. Fix the flaky worktree tests and document the fix")
                }),
            }
        });
        let staff: StaffInputs = if step == 1 {
            draft
                .heads
                .iter()
                .map(|h| {
                    let title = keyed_input(
                        format!("org-wiz-title-{}", h.role),
                        h.title.clone(),
                        window,
                        cx,
                    );
                    (h.role.clone(), title)
                })
                .collect()
        } else {
            vec![]
        };
        let muted = cx.theme().muted_foreground;
        let body = match step {
            0 => self.wizard_templates(draft, cx).into_any_element(),
            1 => self.wizard_staff(draft, &staff, window, cx).into_any_element(),
            2 => self.wizard_rules(&rules, cx).into_any_element(),
            _ => self.wizard_review(draft, &rules, cx).into_any_element(),
        };
        let theme = cx.theme();
        let steps = h_flex().gap(px(6.)).text_size(px(12.)).children(STEPS.iter().enumerate().map(
            |(i, name)| {
                let pill = div().px(px(9.)).py(px(2.)).rounded_full().border_1();
                if i == step {
                    pill.bg(theme.foreground)
                        .border_color(theme.foreground)
                        .text_color(theme.background)
                } else if i < step {
                    pill.border_color(rgb(GREEN)).text_color(rgb(GREEN))
                } else {
                    pill.border_color(theme.border).text_color(muted)
                }
                .child(format!("{} {name}", i + 1))
            },
        ));
        let (back, next) = (staff.clone(), staff);
        let primary = if step < 3 {
            aether_ui::button("org-wiz-next").primary().small().label("Next").on_click(
                cx.listener(move |this, _, _, cx| this.org_wizard_go(step + 1, &next, cx)),
            )
        } else {
            let rules = rules.clone();
            aether_ui::button("org-wiz-create")
                .primary()
                .small()
                .label("Create organization")
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.org_wizard_create(&rules, window, cx)
                }))
        };
        let panel = div()
            .w(px(860.))
            .max_h(window.viewport_size().height * 0.86)
            .rounded(px(14.))
            .child(
                h_flex()
                    .gap(px(10.))
                    .px(px(18.))
                    .py(px(14.))
                    .border_b_1()
                    .border_color(theme.border)
                    .child(heading("org-wiz-title", "Set up organization"))
                    .child(div().flex_1())
                    .child(steps),
            )
            .child(
                div()
                    .id("org-wiz-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(18.))
                    .py(px(16.))
                    .child(body),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .px(px(18.))
                    .py(px(12.))
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        aether_ui::button("org-wiz-cancel")
                            .small()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.org_close(cx))),
                    )
                    .child(div().flex_1())
                    .when(step > 0, |d| {
                        d.child(aether_ui::button("org-wiz-back").small().label("Back").on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.org_wizard_go(step - 1, &back, cx)
                            }),
                        ))
                    })
                    .child(primary),
            );
        Some(self.org_scrim(false, panel, "Set up organization".into(), window, cx))
    }

    fn wizard_templates(&self, draft: &OrgSpec, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border, ring) = (theme.muted_foreground, theme.border, rgb(BLUE));
        let per_head = self.settings.org_workers_per_head;
        let cards = TEMPLATES.iter().map(|t| {
            let on = draft.template == t.id;
            let id = t.id;
            v_flex()
                .id(t.id)
                .flex_1()
                .min_w_0()
                .gap(px(4.))
                .p(px(12.))
                .rounded(px(12.))
                .border_1()
                .border_color(if on { ring.into() } else { border })
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(d) = this.org_wizard_draft() {
                        *d = OrgSpec::from_template(id, per_head);
                    }
                    cx.notify();
                }))
                .child(
                    h_flex()
                        .gap(px(6.))
                        .text_size(px(13.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(t.name)
                        .when(t.id == "startup", |d| {
                            let green: Hsla = rgb(GREEN).into();
                            d.child(pill("recommended".into(), green.opacity(0.15), green))
                        }),
                )
                .child(div().text_size(px(12.)).text_color(muted).child(t.desc))
                .child(
                    h_flex()
                        .pt(px(4.))
                        .gap(px(3.))
                        .flex_wrap()
                        .children(t.roles.iter().map(|r| avatar(r, 22.))),
                )
        });
        v_flex().gap(px(14.)).child(h_flex().gap(px(10.)).items_stretch().children(cards)).child(
            div().text_size(px(12.5)).text_color(muted).child(
                "Every head adds coordination cost. Start small; the CEO will tell you when a head is overloaded.",
            ),
        )
    }

    fn wizard_staff(
        &self,
        draft: &OrgSpec,
        staff: &StaffInputs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let header = staff_row(
            ["", "Role", "Model", "Reviewed by", ""].map(|t| t.into_any_element()),
            border,
        )
        .text_size(px(11.))
        .text_color(muted);
        let roles: Vec<String> = draft.heads.iter().map(|h| h.role.clone()).collect();
        let rows: Vec<_> = draft
            .heads
            .iter()
            .zip(staff)
            .map(|(h, (_, title))| {
                let role = h.role.clone();
                let r = role.clone();
                let agent = self.agent_picker(
                    format!("org-wiz-agent-{role}"),
                    h.provider,
                    &h.model,
                    &h.effort,
                    window,
                    cx,
                    move |this, f, _| {
                        this.org_wizard_head(&r, |h| {
                            f(&mut h.provider, &mut h.model, &mut h.effort)
                        })
                    },
                );
                let options = std::iter::once((None, "You".to_string())).chain(
                    roles.iter().filter(|x| **x != role).map(|x| (Some(x.clone()), x.clone())),
                );
                let r = role.clone();
                let reviewer = select(
                    ElementId::Name(format!("org-wiz-rev-{role}").into()),
                    h.reviewer.clone(),
                    options.collect(),
                    cx,
                    move |this, v, _, _| this.org_wizard_head(&r, |h| h.reviewer = v),
                );
                let r = role.clone();
                let remove =
                    aether_ui::button(ElementId::Name(format!("org-wiz-rm-{role}").into()))
                        .small()
                        .ghost()
                        .label("✕")
                        .tooltip("Remove")
                        .accessibility_label(format!("Remove {} from staff", h.title))
                        .min_w(px(24.))
                        .min_h(px(24.))
                        .disabled(role == "CEO")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(d) = this.org_wizard_draft() {
                                d.heads.retain(|h| h.role != r);
                                for h in &mut d.heads {
                                    if h.reviewer.as_ref() == Some(&r) {
                                        h.reviewer = None;
                                    }
                                }
                            }
                            cx.notify();
                        }));
                staff_row(
                    [
                        avatar(&role, 22.).into_any_element(),
                        field(title).small().into_any_element(),
                        agent,
                        reviewer.into_any_element(),
                        remove.into_any_element(),
                    ],
                    border,
                )
            })
            .collect();
        let free: Vec<(&'static str, String)> = ROLES
            .iter()
            .filter(|r| !roles.iter().any(|x| x == r.0))
            .map(|r| (r.0, r.1.to_string()))
            .collect();
        let app = cx.entity();
        let per_head = self.settings.org_workers_per_head;
        let add = aether_ui::button("org-wiz-add")
            .small()
            .label("+ Add head")
            .dropdown_caret(true)
            .disabled(free.is_empty())
            .dropdown_menu(move |mut menu, _, _| {
                for (role, title) in free.clone() {
                    let app = app.clone();
                    menu = menu.item(PopupMenuItem::new(title).on_click(move |_, _, cx| {
                        app.update(cx, |this, cx| {
                            if let Some(d) = this.org_wizard_draft() {
                                d.heads.push(Head::new(role, per_head));
                            }
                            cx.notify();
                        })
                    }));
                }
                menu
            });
        v_flex().child(header).children(rows).child(
            h_flex().mt(px(10.)).gap(px(8.)).child(add).child(div().flex_1()).child(help(
                "Tip: pick a reviewer from a different vendor than the author.",
                muted,
            )),
        )
    }

    fn wizard_rules(
        &mut self,
        rules: &Entity<RulesForm>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let saved = self.settings.org_pool.take();
        let auto = self.org_pool_cap();
        self.settings.org_pool = saved;
        let f = rules.read(cx);
        let (manual, hire) = (f.manual, f.hire);
        let (pool, builds, per_head, rotate) =
            (f.pool.clone(), f.builds.clone(), f.per_head.clone(), f.rotate.clone());
        let form = rules.clone();
        let pool_pick = select(
            "org-wiz-pool",
            manual,
            vec![(false, format!("Auto from machine ({auto})")), (true, "Manual".into())],
            cx,
            move |_, v, _, cx| form.update(cx, |f, _| f.manual = v),
        );
        let form = rules.clone();
        let hire_pick = select(
            "org-wiz-hire",
            hire,
            OrgHire::ALL.iter().map(|h| (*h, h.label().to_string())).collect(),
            cx,
            move |_, v, _, cx| form.update(cx, |f, _| f.hire = v),
        );
        let small = |input: &Entity<InputState>, w: f32| div().w(px(w)).child(field(input).small());
        v_flex()
            .gap(px(14.))
            .child(row2(
                labeled(
                    "Worker pool",
                    h_flex()
                        .gap(px(8.))
                        .child(div().flex_1().child(pool_pick))
                        .when(manual, |d| d.child(small(&pool, 70.))),
                    muted,
                )
                .child(help("Most workers running at once across every head.", muted)),
                labeled("Builds at a time", small(&builds, 80.), muted).child(help(
                    "cargo/npm builds are serialized; shared target dirs clobber when parallel.",
                    muted,
                )),
            ))
            .child(row2(
                labeled("Workers per head", small(&per_head, 80.), muted),
                labeled("Hiring beyond the per-head cap", hire_pick, muted)
                    .child(help("Rules: has a card, head under its cap, pool has a slot.", muted)),
            ))
            .child(row2(
                labeled(
                    "Rotate a head's session at",
                    h_flex()
                        .gap(px(6.))
                        .child(small(&rotate, 70.))
                        .child(div().text_color(muted).child("% context")),
                    muted,
                )
                .child(help(
                    "Head writes its memory file, then starts fresh with the same role.",
                    muted,
                )),
                div().flex_1(),
            ))
    }

    fn wizard_review(
        &mut self,
        draft: &OrgSpec,
        rules: &Entity<RulesForm>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, fg) = (theme.muted_foreground, theme.foreground);
        let f = rules.read(cx);
        let pool = if f.manual {
            parse_count(&value(&f.pool, cx)).unwrap_or(1).max(1)
        } else {
            let saved = self.settings.org_pool.take();
            let n = self.org_pool_cap();
            self.settings.org_pool = saved;
            n
        };
        let f = rules.read(cx);
        let per_head = value(&f.per_head, cx);
        let builds = parse_count(&value(&f.builds, cx)).unwrap_or(1);
        let goal = f.goal.clone();
        let titles: Vec<&str> = draft.heads.iter().map(|h| h.title.as_str()).collect();
        let title_of = |role: &Option<String>| match role {
            Some(r) => {
                draft.heads.iter().find(|h| &h.role == r).map_or(r.clone(), |h| h.title.clone())
            }
            None => "you".into(),
        };
        let routing: Vec<String> = draft
            .heads
            .iter()
            .filter(|h| h.role != "CEO")
            .map(|h| format!("{} → {}", h.title, title_of(&h.reviewer)))
            .collect();
        let dir = self
            .org_project()
            .and_then(|p| org_dir(&p))
            .map(|d| d.display().to_string())
            .unwrap_or_default();
        let item = |text: String| {
            h_flex()
                .gap(px(8.))
                .items_start()
                .child(div().child("•"))
                .child(div().flex_1().child(text))
        };
        let mut items = vec![
            item(format!(
                "Create {} pinned lead chats in this project: {}.",
                titles.len(),
                titles.join(", ")
            )),
            item(format!("Write role files and a roster to {dir}.")),
        ];
        if !routing.is_empty() {
            items.push(item(format!("Label review routing: {}.", routing.join(", "))));
        }
        items.push(item(format!(
            "Cap workers at {pool} org-wide, {per_head} per head, {builds} build{} at a time.",
            if builds == 1 { "" } else { "s" }
        )));
        items.push(item(
            "Start the CEO. It reads the roster and greets each head. Nothing else runs until you give a goal."
                .into(),
        ));
        v_flex()
            .gap(px(14.))
            .child(div().text_color(fg).child("Aether will:"))
            .child(v_flex().gap(px(6.)).text_color(muted).children(items))
            .child(labeled("First goal (optional)", field(&goal).small(), muted))
    }

    /// Head editor drawer for the head with `role`.
    pub(crate) fn render_org_head_editor(
        &mut self,
        role: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let org = self.org_current()?.clone();
        let adding = role.is_empty();
        let head = if adding {
            let free = ROLES.iter().find(|r| org.head(r.0).is_none())?;
            Head::new(free.0, self.settings.org_workers_per_head)
        } else {
            org.head(role)?.clone()
        };
        let dir = org_dir(&org.project);
        let key = format!("org-head-{}", if adding { "+" } else { role });
        let form = window.use_keyed_state(ElementId::Name(key.into()), cx, move |window, cx| {
            let read = |path: Option<std::path::PathBuf>| {
                path.and_then(|p| std::fs::read_to_string(p).ok())
            };
            let role_text = read(dir.as_ref().map(|d| role_path(d, &head.role)))
                .unwrap_or_else(|| role_file_text(&head));
            let memory = read(dir.as_ref().map(|d| memory_path(d, &head.role))).unwrap_or_default();
            HeadForm {
                title: new_input(head.title.clone(), window, cx),
                owns: new_input(head.owns.clone(), window, cx),
                workers: new_input(head.max_workers.to_string(), window, cx),
                role_text: cx.new(|cx| {
                    let mut s = TextareaState::new(window, cx).auto_grow(10, 18);
                    s.set_value(role_text, window, cx);
                    s
                }),
                memory,
                head,
            }
        });
        let f = form.read(cx);
        let head = f.head.clone();
        let (title, owns, workers, role_text, memory) = (
            f.title.clone(),
            f.owns.clone(),
            f.workers.clone(),
            f.role_text.clone(),
            f.memory.clone(),
        );
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let file = |path: Option<std::path::PathBuf>| {
            path.map(|p| p.display().to_string()).unwrap_or_default()
        };
        let dir = org_dir(&org.project);
        let role_file = file(dir.as_ref().map(|d| role_path(d, &head.role)));
        let memory_file = file(dir.as_ref().map(|d| memory_path(d, &head.role)));

        let reviewers = std::iter::once((None, "You".to_string())).chain(
            org.heads
                .iter()
                .filter(|h| h.role != head.role)
                .map(|h| (Some(h.role.clone()), h.title.clone())),
        );
        let fm = form.clone();
        let reviewer = select(
            "org-head-rev",
            head.reviewer.clone(),
            reviewers.collect(),
            cx,
            move |_, v, _, cx| fm.update(cx, |f, _| f.head.reviewer = v),
        );
        let fm = form.clone();
        let agent = self.agent_picker(
            "org-head-agent".into(),
            head.provider,
            &head.model,
            &head.effort,
            window,
            cx,
            move |_, f, cx| {
                fm.update(cx, |form, _| {
                    let h = &mut form.head;
                    f(&mut h.provider, &mut h.model, &mut h.effort)
                })
            },
        );
        let role_pick = adding.then(|| {
            let free: Vec<(String, String)> = ROLES
                .iter()
                .filter(|r| org.head(r.0).is_none())
                .map(|r| (r.0.to_string(), r.1.to_string()))
                .collect();
            let fm = form.clone();
            let per_head = self.settings.org_workers_per_head;
            labeled(
                "Role",
                select("org-head-role", head.role.clone(), free, cx, move |_, r, window, cx| {
                    let h = Head::new(&r, per_head);
                    let f = fm.read(cx);
                    let (t, o, w, rt) =
                        (f.title.clone(), f.owns.clone(), f.workers.clone(), f.role_text.clone());
                    set(&t, h.title.clone(), window, cx);
                    set(&o, h.owns.clone(), window, cx);
                    set(&w, h.max_workers.to_string(), window, cx);
                    let text = role_file_text(&h);
                    rt.update(cx, |s, cx| s.set_value(text, window, cx));
                    fm.update(cx, |f, _| f.head = h);
                }),
                muted,
            )
        });
        let pinned: Hsla = rgb(0xb18cff).into();
        let memory_view = if memory.trim().is_empty() {
            div()
                .text_color(muted)
                .child("Nothing written yet. The head adds notes here as it works.")
        } else {
            div().whitespace_normal().child(memory)
        };
        let body = v_flex()
            .id("org-head-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(18.))
            .py(px(16.))
            .gap(px(14.))
            .children(role_pick)
            .child(row2(
                labeled("Title", field(&title).small(), muted),
                labeled("Reviewed by", reviewer, muted),
            ))
            .child(row2(
                labeled(
                    h_flex().gap(px(6.)).child("Model").child(pill("pinned".into(), pinned.opacity(0.15), pinned)),
                    agent,
                    muted,
                )
                .child(help("Changing it hands the session off; the role and memory carry over.", muted)),
                labeled("Max workers", field(&workers).small(), muted),
            ))
            .child(labeled("Owns", field(&owns).small(), muted))
            .child(
                labeled(format!("Role file  {role_file}"), field_area(&role_text), muted).child(help(
                    "Sent at the start of every fresh session, so it survives rotation and handoff.",
                    muted,
                )),
            )
            .child(labeled(
                format!("Memory, written by the head  {memory_file}"),
                div()
                    .p(px(10.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(border)
                    .text_size(px(12.))
                    .child(memory_view),
                muted,
            ));

        let r = head.role.clone();
        let chat = head.chat;
        let save = {
            let form = form.clone();
            cx.listener(move |this, _, window, cx| {
                let f = form.read(cx);
                let mut h = f.head.clone();
                let title = value(&f.title, cx);
                if !title.is_empty() {
                    h.title = title;
                }
                h.owns = value(&f.owns, cx);
                h.max_workers = parse_count(&value(&f.workers, cx)).unwrap_or(h.max_workers);
                let text = f.role_text.read(cx).value().to_string();
                let result = if adding {
                    this.org_add_head_with_role(h, text, window, cx)
                } else {
                    this.org_update_head(h, Some(text), window, cx)
                };
                match result {
                    Ok(()) => this.org_close(cx),
                    Err(e) => report::<()>(Err(e), window, cx),
                }
            })
        };
        let footer = h_flex()
            .gap(px(8.))
            .px(px(18.))
            .py(px(12.))
            .border_t_1()
            .border_color(border)
            .when(!adding, |d| {
                let (r1, r2) = (r.clone(), r.clone());
                d.child(
                    aether_ui::button("org-head-remove")
                        .small()
                        .danger()
                        .label("Remove head")
                        .disabled(r == "CEO")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let result = this.org_remove_head(&r1, window, cx);
                            report(result, window, cx);
                        })),
                )
                .child(
                    aether_ui::button("org-head-rotate").small().label("Rotate session").on_click(
                        cx.listener(move |this, _, window, cx| {
                            let result = this.org_rotate(&r2, window, cx);
                            report(result, window, cx);
                        }),
                    ),
                )
            })
            .child(div().flex_1())
            .when_some(chat, |d, chat| {
                d.child(aether_ui::button("org-head-chat").small().label("Open chat").on_click(
                    cx.listener(move |this, _, window, cx| {
                        this.org.overlay = OrgOverlay::None;
                        this.board_open_chat(chat, window, cx);
                    }),
                ))
            })
            .child(
                aether_ui::button("org-head-save")
                    .primary()
                    .small()
                    .label(if adding { "Add head" } else { "Save" })
                    .on_click(save),
            );
        let sub = match chat {
            Some(id) => format!("chat #{id} · pinned lead"),
            None => "new head".into(),
        };
        let panel = div()
            .w(px(520.))
            .h_full()
            .child(
                h_flex()
                    .gap(px(10.))
                    .px(px(18.))
                    .py(px(14.))
                    .border_b_1()
                    .border_color(border)
                    .child(avatar(&head.role, 30.))
                    .child(
                        v_flex()
                            .child(heading("org-head-title", head.title.clone()))
                            .child(div().text_size(px(12.)).text_color(muted).child(sub)),
                    )
                    .child(div().flex_1())
                    .child(
                        aether_ui::button("org-head-close")
                            .small()
                            .label("Close")
                            .accessibility_label("Close head editor")
                            .min_w(px(24.))
                            .min_h(px(24.))
                            .on_click(cx.listener(|this, _, _, cx| this.org_close(cx))),
                    ),
            )
            .child(body)
            .child(footer);
        Some(self.org_scrim(
            true,
            panel,
            format!("{} head: {}", if adding { "Add" } else { "Edit" }, head.title),
            window,
            cx,
        ))
    }

    /// Writes the edited role file first so onboarding sends it, then adds the head.
    fn org_add_head_with_role(
        &mut self,
        head: Head,
        role_text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let project = self.org_project().ok_or("open a project chat first")?;
        let path = role_path(&org_dir(&project).ok_or("no config folder")?, &head.role);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, role_text).map_err(|e| e.to_string())?;
        self.org_add_head(head, window, cx).map(|_| ())
    }

    fn org_hire_draft(&mut self) -> Option<&mut HireSpec> {
        match &mut self.org.overlay {
            OrgOverlay::Hire(d) => Some(d),
            _ => None,
        }
    }

    /// Hire dialog; Hire calls `org_hire`.
    pub(crate) fn render_org_hire(
        &mut self,
        draft: &HireSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let org = self.org_current()?.clone();
        let heads: Vec<(String, String)> = org
            .heads
            .iter()
            .filter(|h| h.role != "CEO")
            .map(|h| (h.role.clone(), h.title.clone()))
            .collect();
        let head = org
            .head(&draft.head)
            .filter(|h| h.role != "CEO")
            .or_else(|| heads.first().and_then(|(r, _)| org.head(r)));
        let task = window.use_keyed_state("org-hire-task", cx, |window, cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 8)
                .placeholder("What the worker should do, and how you will know it is done")
        });
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let current = head.map(|h| h.role.clone()).unwrap_or_default();
        let head_pick = select("org-hire-head", current.clone(), heads, cx, |this, r, _, _| {
            if let Some(d) = this.org_hire_draft() {
                d.head = r;
            }
        });
        let cards = std::iter::once((None, "New card".to_string())).chain(
            self.cards
                .iter()
                .filter(|c| c.project == org.project && c.col != Col::Done)
                .map(|c| (Some(c.id), format!("#{} {}", c.id, c.title))),
        );
        let card_pick =
            select("org-hire-card", draft.card, cards.collect(), cx, |this, c, _, _| {
                if let Some(d) = this.org_hire_draft() {
                    d.card = c;
                }
            });
        let chosen = (draft.provider, draft.model.clone());
        let model_pick = self.agent_picker(
            "org-hire-agent".into(),
            chosen.0,
            &chosen.1,
            &draft.effort,
            window,
            cx,
            |this, f, _| {
                if let Some(d) = this.org_hire_draft() {
                    f(&mut d.provider, &mut d.model, &mut d.effort)
                }
            },
        );
        let (busy, cap) = (self.org_pool_busy(), self.org_pool_cap());
        let mut checks = vec![];
        if let Some(h) = head {
            let running = self
                .chats
                .iter()
                .filter(|c| c.parent.is_some() && c.parent == h.chat && !c.archived && c.busy())
                .count();
            checks = hire_checks(h, draft.card, running, busy, cap);
        }
        let free = busy < cap;
        let body = v_flex()
            .px(px(18.))
            .py(px(16.))
            .gap(px(14.))
            .child(row2(labeled("For head", head_pick, muted), labeled("Card", card_pick, muted)))
            .child(labeled("Model", model_pick, muted).child(help(
                "Interns use the cheap tier. Their work is reviewed by the head, then the head's reviewer.",
                muted,
            )))
            .child(labeled("Task", field_area(&task), muted))
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .text_size(px(12.5))
                    .child(Checkbox::new("org-hire-worktree")
                        .accessibility_label("Run worker in isolated worktree")
                        .min_w(px(24.))
                        .min_h(px(24.))
                        .checked(draft.worktree).on_click(
                        cx.listener(|this, checked: &bool, _, cx| {
                            if let Some(d) = this.org_hire_draft() {
                                d.worktree = *checked;
                            }
                            cx.notify();
                        }),
                    ))
                    .child("Own worktree"),
            )
            .child(h_flex().gap(px(6.)).flex_wrap().children(checks.into_iter().map(|(t, ok)| check_pill(t, ok))));
        let role = current.clone();
        let hire = cx.listener(move |this, _, window, cx| {
            let Some(mut spec) = this.org_hire_draft().cloned() else { return };
            let text = task.read(cx).value().trim().to_string();
            if text.is_empty() {
                return report::<()>(Err("Describe the task first.".into()), window, cx);
            }
            spec.head = role.clone();
            (spec.provider, spec.model) = chosen.clone();
            let card = spec.card.and_then(|id| this.cards.iter().find(|c| c.id == id));
            spec.title = card.map_or_else(
                || text.lines().next().unwrap_or_default().chars().take(60).collect(),
                |c| c.title.clone(),
            );
            spec.task = text;
            match this.org_hire(spec, window, cx) {
                Ok(Some(_)) => this.org_close(cx),
                Ok(None) => {
                    window.push_notification(
                        Notification::warning("Hire queued: it starts when a worker slot frees."),
                        cx,
                    );
                    this.org_close(cx)
                }
                Err(e) => report::<()>(Err(e), window, cx),
            }
        });
        let panel = div()
            .w(px(520.))
            .rounded(px(14.))
            .child(
                h_flex()
                    .px(px(18.))
                    .py(px(14.))
                    .border_b_1()
                    .border_color(border)
                    .child(heading("org-hire-title", "Hire a worker")),
            )
            .child(body)
            .child(
                h_flex()
                    .gap(px(8.))
                    .justify_end()
                    .px(px(18.))
                    .py(px(12.))
                    .border_t_1()
                    .border_color(border)
                    .child(
                        aether_ui::button("org-hire-cancel")
                            .small()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.org_close(cx))),
                    )
                    .child(
                        aether_ui::button("org-hire-go")
                            .primary()
                            .small()
                            .label(if free { "Hire" } else { "Queue hire" })
                            .disabled(head.is_none())
                            .on_click(hire),
                    ),
            );
        Some(self.org_scrim(false, panel, "Hire a worker".into(), window, cx))
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/org/dialogs.rs"]
mod tests;
