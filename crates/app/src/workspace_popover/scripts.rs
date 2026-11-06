use std::path::Path;

use aether_core::ProjectScript;
use aether_ui::{field, icon_chip, theme};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    Anchor, AnyElement, Context, FocusHandle, SharedString, Window, div, prelude::*, px,
};

use super::model::{
    PopRows, script_action, script_default_name, script_menu_label, script_name_taken,
    script_row_label,
};
use super::rows::{Where, chip, icon, row, text};
use super::{PopConfirm, PopView, SCRIPT_FORM_CONTEXT};
use crate::app::ChatApp;
use crate::open_in::project_key;

fn script_key(at: &Where) -> Option<String> {
    at.project.as_deref().or(at.folder.as_deref()).map(project_key)
}

impl ChatApp {
    pub(super) fn pop_project_scripts(&self, at: &Where) -> Vec<ProjectScript> {
        let scripts = script_key(at).and_then(|key| self.settings.project_scripts.get(&key));
        scripts.cloned().unwrap_or_default()
    }

    pub(crate) fn run_script(
        &mut self,
        ix: usize,
        script: &ProjectScript,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let chat = &self.chats[ix];
        let (id, Some(folder)) = (chat.id, self.folder_of(chat)) else { return false };
        let Some(view) = self.drawer_for(ix, window, cx) else { return false };
        if !self.script_runs.start(&folder, &script.name, id) {
            return false;
        }
        let name = SharedString::from(script.name.clone());
        let command = script.command.clone();
        view.update(cx, |t, cx| t.run_command(name.clone(), name, command, window, cx));
        true
    }

    pub(crate) fn stop_script(&mut self, folder: &Path, name: &str, cx: &mut Context<Self>) {
        let Some(chat) = self.script_runs.running(folder, name) else { return };
        match self.terminals.get(&chat).map(|d| d.view.clone()) {
            Some(view) if view.read(cx).command_running(name, cx) => {
                view.update(cx, |t, cx| t.stop_command(name, cx));
            }
            _ => self.script_runs.end(folder, name),
        }
        cx.notify();
    }

    fn stop_project_script(&mut self, key: &str, name: &str, cx: &mut Context<Self>) {
        for (folder, chat) in self.script_runs.runs_of(name) {
            let ix = self.chats.iter().position(|c| c.id == chat);
            if ix.and_then(|ix| script_key(&self.where_of(ix))).as_deref() == Some(key) {
                self.stop_script(&folder, name, cx);
            }
        }
    }

    pub(crate) fn stop_chat_scripts(&mut self, chat: i64, cx: &mut Context<Self>) {
        let runs = self.script_runs.drop_chat(chat);
        if let Some(view) = self.terminals.get(&chat).map(|d| d.view.clone()) {
            view.update(cx, |t, cx| runs.iter().for_each(|(_, name)| t.stop_command(name, cx)));
        }
    }

    fn toggle_script(
        &mut self,
        script: &ProjectScript,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(folder) = self.folder_of(&self.chats[self.active]) else { return };
        if self.script_runs.running(&folder, &script.name).is_some() {
            self.stop_script(&folder, &script.name, cx);
        } else if self.run_script(self.active, script, window, cx) {
            self.close_workspace_pop(window, cx);
        }
    }

    fn open_script_form(&mut self, ix: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let script = ix.and_then(|i| self.pop_project_scripts(&self.pop_where()).get(i).cloned());
        let script = script.unwrap_or(ProjectScript {
            name: String::new(),
            command: String::new(),
            on_new_worktree: false,
        });
        let pop = &mut self.workspace_pop;
        pop.script_name.update(cx, |s, cx| s.set_value(script.name, window, cx));
        pop.script_cmd.update(cx, |s, cx| s.set_value(script.command, window, cx));
        pop.script_setup = script.on_new_worktree;
        self.pop_view(PopView::Script(ix), window, cx);
    }

    fn save_script(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let PopView::Script(edit) = self.workspace_pop.view else { return };
        let pop = &self.workspace_pop;
        let command = pop.script_cmd.read(cx).value().trim().to_string();
        let Some(key) = script_key(&self.pop_where()).filter(|_| !command.is_empty()) else {
            return;
        };
        let name = pop.script_name.read(cx).value().trim().to_string();
        let name = if name.is_empty() { script_default_name(&command) } else { name };
        let on_new_worktree = pop.script_setup;
        let list = self.settings.project_scripts.get(&key).map_or(&[][..], Vec::as_slice);
        if script_name_taken(list, &name, edit) {
            self.workspace_pop.error = Some(format!("A script named {name} already exists").into());
            return cx.notify();
        }
        let renamed = edit.and_then(|i| list.get(i)).map(|s| s.name.clone());
        if let Some(old) = renamed.filter(|old| *old != name) {
            self.stop_project_script(&key, &old, cx);
        }
        self.update_settings(cx, |s| {
            let list = s.project_scripts.entry(key).or_default();
            let script = ProjectScript { name, command, on_new_worktree };
            match edit.filter(|&i| i < list.len()) {
                Some(i) => list[i] = script,
                None => list.push(script),
            }
        });
        self.pop_view(PopView::Main, window, cx);
    }

    fn delete_script(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let at = self.pop_where();
        let name = self.pop_project_scripts(&at).get(ix).map(|s| s.name.clone());
        let Some(key) = script_key(&at) else { return };
        if let Some(name) = name {
            self.stop_project_script(&key, &name, cx);
        }
        self.update_settings(cx, |s| {
            if let Some(list) = s.project_scripts.get_mut(&key)
                && ix < list.len()
            {
                list.remove(ix);
            }
        });
        self.pop_view(PopView::Main, window, cx);
    }

    pub(super) fn pop_script_rows(
        &self,
        at: &Where,
        state: &PopRows,
        scripts: Vec<ProjectScript>,
        h: &[FocusHandle],
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let folder = self.folder_of(&self.chats[self.active]);
        let off = state.reason.filter(|_| !state.scripts);
        let muted = cx.theme().muted_foreground;
        let mono = cx.theme().mono_font_family.clone();
        let danger = theme::danger_text(cx);
        let view = cx.entity();
        let mut out = vec![
            div()
                .px(px(10.))
                .pt(px(6.))
                .text_xs()
                .text_color(muted)
                .child("Scripts")
                .into_any_element(),
        ];
        let n = scripts.len();
        for (i, script) in scripts.into_iter().enumerate() {
            let running = folder
                .as_deref()
                .is_some_and(|f| self.script_runs.running(f, &script.name).is_some());
            let glyph = if running {
                Icon::new(Lucide::Square).text_color(danger)
            } else {
                Icon::new(Lucide::Play)
            };
            let group = SharedString::from(format!("pop-script-{i}"));
            let menu = {
                let view = view.clone();
                move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
                    let (edit, delete) = (view.clone(), view.clone());
                    menu.item(PopupMenuItem::new("Edit").icon(Lucide::Pencil).on_click(
                        move |_, window, cx| {
                            edit.update(cx, |this, cx| this.open_script_form(Some(i), window, cx))
                        },
                    ))
                    .item(
                        PopupMenuItem::new("Delete").icon(Lucide::Trash).on_click(
                            move |_, window, cx| {
                                delete.update(cx, |this, cx| this.delete_script(i, window, cx))
                            },
                        ),
                    )
                }
            };
            let (run, chip_run) = (script.clone(), script.clone());
            out.push(
                row(("pop-script", i), &h[i], script_row_label(&script, running).into(), off, cx)
                    .group(group.clone())
                    .py(px(4.))
                    .pl(px(6.))
                    .child(
                        icon_chip(("pop-script-run", i), false, cx)
                            .icon(glyph)
                            .accessibility_label(script_action(&script.name, running))
                            .tooltip(if running { "Stop script" } else { "Run script" })
                            .tab_stop(false)
                            .disabled(off.is_some())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.toggle_script(&chip_run, window, cx)
                            })),
                    )
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(6.))
                            .items_center()
                            .child(div().min_w_0().truncate().child(script.name.clone()))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(mono.clone())
                                    .text_xs()
                                    .text_color(muted)
                                    .child(script.command.clone()),
                            ),
                    )
                    .when(script.on_new_worktree, |r| r.child(chip("on new worktree", cx)))
                    .child(
                        div()
                            .id(("pop-script-more-slot", i))
                            .flex_none()
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(
                                aether_ui::button(("pop-script-more", i))
                                    .ghost()
                                    .small()
                                    .icon(Lucide::Ellipsis)
                                    .accessibility_label(script_menu_label(&script.name))
                                    .tooltip("Edit or delete")
                                    .opacity(0.)
                                    .group_hover(group, |s| s.opacity(1.))
                                    .focus_visible(|s| s.opacity(1.))
                                    .dropdown_menu_with_anchor(Anchor::TopRight, menu),
                            ),
                    )
                    .when_some(off, |r, why| {
                        r.tooltip(move |window, cx| Tooltip::new(why).build(window, cx))
                    })
                    .when(off.is_none(), |r| {
                        r.on_click(cx.listener(move |this, _, window, cx| {
                            this.toggle_script(&run, window, cx)
                        }))
                    })
                    .into_any_element(),
            );
        }
        let add_off = script_key(at).is_none().then_some("Folder not found");
        out.push(
            row("pop-script-add", &h[n], "Add project script".into(), add_off, cx)
                .child(icon(Lucide::Plus, cx))
                .child(text("Add project script", add_off.map(Into::into), cx))
                .when(add_off.is_none(), |r| {
                    r.on_click(
                        cx.listener(|this, _, window, cx| this.open_script_form(None, window, cx)),
                    )
                })
                .into_any_element(),
        );
        out
    }

    pub(super) fn pop_script_form(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        self.workspace_pop.rows(1, cx);
        let pop = &self.workspace_pop;
        let ready = !pop.script_cmd.read(cx).value().trim().is_empty();
        let muted = cx.theme().muted_foreground;
        let mono = cx.theme().mono_font_family.clone();
        let label = |t: &'static str| div().text_xs().text_color(muted).child(t);
        vec![
            v_flex()
                .gap(px(4.))
                .px(px(10.))
                .pb(px(4.))
                .key_context(SCRIPT_FORM_CONTEXT)
                .on_action(
                    cx.listener(|this, _: &PopConfirm, window, cx| this.save_script(window, cx)),
                )
                .child(label("Name"))
                .child(field(&pop.script_name).small().aria_label("Name"))
                .child(label("Command"))
                .child(
                    div()
                        .font_family(mono)
                        .child(field(&pop.script_cmd).small().aria_label("Command")),
                )
                .into_any_element(),
            div()
                .px(px(10.))
                .py(px(4.))
                .child(
                    Checkbox::new("pop-script-setup")
                        .label("Run automatically when a new worktree is created")
                        .checked(pop.script_setup)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.workspace_pop.script_setup = *checked;
                            cx.notify();
                        })),
                )
                .into_any_element(),
            h_flex()
                .gap(px(8.))
                .items_center()
                .px(px(10.))
                .pt(px(6.))
                .pb(px(2.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(muted)
                        .child("Runs in this chat's folder. Output goes to the terminal."),
                )
                .child(
                    aether_ui::button("pop-script-save")
                        .primary()
                        .label("Save")
                        .disabled(!ready)
                        .on_click(cx.listener(|this, _, window, cx| this.save_script(window, cx))),
                )
                .into_any_element(),
        ]
    }
}
