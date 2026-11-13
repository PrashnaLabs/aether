use std::rc::Rc;

use aether_core::{OrgHire, Provider};
use aether_ui::{field, field_area};
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Context, Div, ElementId, Entity, FontWeight, Hsla, MouseButton, Role, Window,
    div, prelude::*, px, rgb,
};

use crate::app::{CLIS, ChatApp};
use crate::board::model::Col;
use crate::org::model::{
    Head, HireSpec, OrgSpec, ROLES, TEMPLATES, hire_checks, memory_path, models, role_file_text,
    role_path,
};
use crate::org::view::shared::{avatar, report};
use crate::org::{OrgOverlay, org_dir};
use crate::project_page::view::pill;

const STEPS: [&str; 4] = ["Template", "Staff", "Rules", "Review"];
const GREEN: u32 = 0x3fb950;
const RED: u32 = 0xf85149;
const BLUE: u32 = 0x58a6ff;

type Pick<T> = Rc<dyn Fn(&mut ChatApp, T, &mut Window, &mut Context<ChatApp>)>;
type StaffInputs = Vec<(String, Entity<InputState>, Entity<InputState>)>;

/// Dollar amount typed as `12`, `12.5` or `$12`.
pub(crate) fn parse_usd(text: &str) -> Option<f64> {
    text.trim().trim_start_matches('$').trim().parse().ok().filter(|v: &f64| *v >= 0.)
}

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
    budget: Entity<InputState>,
    rotate: Entity<InputState>,
    goal: Entity<InputState>,
}

struct HeadForm {
    head: Head,
    title: Entity<InputState>,
    owns: Entity<InputState>,
    budget: Entity<InputState>,
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

fn usd(v: f64) -> String {
    format!("${v}")
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

fn staff_row(cells: [AnyElement; 7], border: Hsla) -> Div {
    let widths = [Some(22.), None, Some(130.), Some(190.), Some(120.), Some(80.), Some(28.)];
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

impl ChatApp {
    fn org_close(&mut self, cx: &mut Context<Self>) {
        self.org.overlay = OrgOverlay::None;
        cx.notify();
    }

    /// Dimmed backdrop; a click on it closes the overlay. `drawer` docks the panel right.
    fn org_scrim(&self, drawer: bool, panel: Div, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("org-scrim")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .bg(gpui_kit::black().opacity(0.55))
            .map(|d| if drawer { d.justify_end() } else { d.items_center().justify_center() })
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.org_close(cx)))
            .child(
                panel
                    .occlude()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .bg(cx.theme().background)
                    .border_1()
                    .border_color(cx.theme().border)
                    .text_size(px(13.)),
            )
            .into_any_element()
    }

    /// Installed CLI providers, `current` always included.
    fn org_providers(&self, current: Provider) -> Vec<Provider> {
        let mut out: Vec<Provider> = CLIS
            .into_iter()
            .filter(|p| self.probes.get(p).is_none_or(|probe| probe.installed))
            .collect();
        if !out.contains(&current) {
            out.insert(0, current);
        }
        out
    }

    /// Probed models of `provider`, else the built-in list; `current` always included.
    fn org_models(&self, provider: Provider, current: &str) -> Vec<String> {
        let mut out: Vec<String> = match self.probes.get(&provider) {
            Some(p) if !p.models.is_empty() => p.models.iter().map(|m| m.id.clone()).collect(),
            _ => models(provider).iter().map(|m| m.to_string()).collect(),
        };
        if !current.is_empty() && !out.iter().any(|m| m == current) {
            out.insert(0, current.into());
        }
        out
    }

    fn provider_select(
        &self,
        id: ElementId,
        current: Provider,
        cx: &mut Context<Self>,
        pick: impl Fn(&mut ChatApp, Provider, String, &mut Window, &mut Context<ChatApp>) + 'static,
    ) -> AnyElement {
        let options = self.org_providers(current).into_iter().map(|p| (p, p.label().into()));
        select(id, current, options.collect(), cx, move |this, p, window, cx| {
            let model = this.org_models(p, "").into_iter().next().unwrap_or_default();
            pick(this, p, model, window, cx)
        })
    }

    fn model_select(
        &self,
        id: ElementId,
        provider: Provider,
        current: &str,
        cx: &mut Context<Self>,
        pick: impl Fn(&mut ChatApp, String, &mut Window, &mut Context<ChatApp>) + 'static,
    ) -> AnyElement {
        let options = self.org_models(provider, current).into_iter().map(|m| (m.clone(), m));
        select(id, current.to_string(), options.collect(), cx, pick)
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
            for (role, title, budget) in staff {
                let Some(h) = draft.heads.iter_mut().find(|h| &h.role == role) else { continue };
                let title = value(title, cx);
                if !title.is_empty() {
                    h.title = title;
                }
                if let Some(v) = parse_usd(&value(budget, cx)) {
                    h.budget_usd = v;
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
        let budget = parse_usd(&value(&f.budget, cx));
        let rotate = parse_count(&value(&f.rotate, cx)).map(|n| n.clamp(10, 100) as u8);
        let hire = f.hire;
        spec.goal = value(&f.goal, cx);
        self.update_settings(cx, |s| {
            s.org_pool = pool;
            s.org_hire = hire;
            s.org_builds = builds.unwrap_or(s.org_builds).max(1);
            s.org_workers_per_head = per_head.unwrap_or(s.org_workers_per_head);
            s.org_budget_usd = budget.unwrap_or(s.org_budget_usd);
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
        let init = (
            s.org_pool,
            s.org_builds,
            s.org_workers_per_head,
            s.org_hire,
            s.org_budget_usd,
            s.org_rotate_at,
        );
        let rules = window.use_keyed_state("org-wizard-rules", cx, move |window, cx| {
            let (pool, builds, per_head, hire, budget, rotate) = init;
            RulesForm {
                manual: pool.is_some(),
                hire,
                pool: new_input(pool.unwrap_or(3).to_string(), window, cx),
                builds: new_input(builds.to_string(), window, cx),
                per_head: new_input(per_head.to_string(), window, cx),
                budget: new_input(usd(budget), window, cx),
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
                    let budget = keyed_input(
                        format!("org-wiz-budget-{}", h.role),
                        usd(h.budget_usd),
                        window,
                        cx,
                    );
                    (h.role.clone(), title, budget)
                })
                .collect()
        } else {
            vec![]
        };
        let muted = cx.theme().muted_foreground;
        let body = match step {
            0 => self.wizard_templates(draft, cx).into_any_element(),
            1 => self.wizard_staff(draft, &staff, cx).into_any_element(),
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
        Some(self.org_scrim(false, panel, cx))
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
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let header = staff_row(
            ["", "Role", "Provider", "Model", "Reviewed by", "Budget/day", ""]
                .map(|t| t.into_any_element()),
            border,
        )
        .text_size(px(11.))
        .text_color(muted);
        let roles: Vec<String> = draft.heads.iter().map(|h| h.role.clone()).collect();
        let rows: Vec<_> = draft
            .heads
            .iter()
            .zip(staff)
            .map(|(h, (_, title, budget))| {
                let role = h.role.clone();
                let r = role.clone();
                let provider = self.provider_select(
                    ElementId::Name(format!("org-wiz-prov-{role}").into()),
                    h.provider,
                    cx,
                    move |this, p, m, _, _| {
                        this.org_wizard_head(&r, |h| {
                            h.provider = p;
                            h.model = m;
                        })
                    },
                );
                let r = role.clone();
                let model = self.model_select(
                    ElementId::Name(format!("org-wiz-model-{role}").into()),
                    h.provider,
                    &h.model,
                    cx,
                    move |this, m, _, _| this.org_wizard_head(&r, |h| h.model = m),
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
                        provider.into_any_element(),
                        model.into_any_element(),
                        reviewer.into_any_element(),
                        field(budget).small().into_any_element(),
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
        let (pool, builds, per_head, budget, rotate) = (
            f.pool.clone(),
            f.builds.clone(),
            f.per_head.clone(),
            f.budget.clone(),
            f.rotate.clone(),
        );
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
                    .child(help("Rules: has a card, pool has a slot, head under budget.", muted)),
            ))
            .child(row2(
                labeled("Org budget per day", small(&budget, 90.), muted)
                    .child(help("Heads pause and ask you at 100%.", muted)),
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
                budget: new_input(usd(head.budget_usd), window, cx),
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
        let (title, owns, budget, workers, role_text, memory) = (
            f.title.clone(),
            f.owns.clone(),
            f.budget.clone(),
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
        let provider = self.provider_select(
            "org-head-prov".into(),
            head.provider,
            cx,
            move |_, p, m, _, cx| {
                fm.update(cx, |f, _| {
                    f.head.provider = p;
                    f.head.model = m;
                })
            },
        );
        let fm = form.clone();
        let model = self.model_select(
            "org-head-model".into(),
            head.provider,
            &head.model,
            cx,
            move |_, m, _, cx| fm.update(cx, |f, _| f.head.model = m),
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
                    let (t, o, b, w, rt) = (
                        f.title.clone(),
                        f.owns.clone(),
                        f.budget.clone(),
                        f.workers.clone(),
                        f.role_text.clone(),
                    );
                    set(&t, h.title.clone(), window, cx);
                    set(&o, h.owns.clone(), window, cx);
                    set(&b, usd(h.budget_usd), window, cx);
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
                labeled("Provider", provider, muted),
                labeled(
                    h_flex().gap(px(6.)).child("Model").child(pill("pinned".into(), pinned.opacity(0.15), pinned)),
                    model,
                    muted,
                )
                .child(help("Changing it hands the session off; the role and memory carry over.", muted)),
            ))
            .child(row2(
                labeled("Budget per day", field(&budget).small(), muted),
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
                h.budget_usd = parse_usd(&value(&f.budget, cx)).unwrap_or(h.budget_usd);
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
                            .on_click(cx.listener(|this, _, _, cx| this.org_close(cx))),
                    ),
            )
            .child(body)
            .child(footer);
        Some(self.org_scrim(true, panel, cx))
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
        let providers = self.org_providers(head.map_or(Provider::Claude, |h| h.provider));
        let options: Vec<((Provider, String), String)> = hire_models(&providers)
            .into_iter()
            .map(|(p, m)| ((p, m.to_string()), format!("{} / {m}", p.key())))
            .collect();
        let chosen = if draft.model.is_empty() {
            options.first().map(|o| o.0.clone()).unwrap_or((draft.provider, String::new()))
        } else {
            (draft.provider, draft.model.clone())
        };
        let model_pick =
            select("org-hire-model", chosen.clone(), options, cx, |this, (p, m), _, _| {
                if let Some(d) = this.org_hire_draft() {
                    d.provider = p;
                    d.model = m;
                }
            });
        let (busy, cap) = (self.org_pool_busy(), self.org_pool_cap());
        let mut checks = vec![];
        if let Some(h) = head {
            let running = self
                .chats
                .iter()
                .filter(|c| c.parent.is_some() && c.parent == h.chat && !c.archived && c.busy())
                .count();
            checks = hire_checks(h, draft.card, running, busy, cap);
            let spent = h.chat.map_or(0., |id| self.org_spend(id));
            checks.push((
                format!("Budget left ${:.2}", (h.budget_usd - spent).max(0.)),
                spent < h.budget_usd,
            ));
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
                    .child(Checkbox::new("org-hire-worktree").checked(draft.worktree).on_click(
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
        Some(self.org_scrim(false, panel, cx))
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/org/dialogs.rs"]
mod tests;
