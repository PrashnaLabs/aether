use aether_agent::skills;
use aether_core::{Skill, SkillSource};
use aether_ui::theme::Corners;
use aether_ui::{
    Markdown, Segmented, SettingsPage, SettingsRow, SettingsSection, field, field_area,
};
use gpui_kit::component::{
    ActiveTheme, IconName, Sizable, StyledExt,
    button::ButtonVariants,
    h_flex,
    input::{InputState, TextareaState},
    switch::Switch,
    v_flex,
};
use gpui_kit::rems;
use std::ops::Range;

use gpui_kit::{App, Context, Entity, SharedString, Window, div, prelude::*, px, uniform_list};

use crate::app::{ChatApp, Page};

pub(crate) struct SkillEditor {
    original: Option<Skill>,
    name: Entity<InputState>,
    description: Entity<TextareaState>,
    instructions: Entity<TextareaState>,
    error: Option<SharedString>,
    confirm_delete: bool,
    preview: bool,
}

impl ChatApp {
    pub(crate) fn edit_skill(
        &mut self,
        original: Option<Skill>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (name, description, instructions) = original
            .as_ref()
            .map(|s| (s.name.clone(), s.description.clone(), s.instructions.clone()))
            .unwrap_or_default();
        let name = cx.new(|cx| {
            let mut input = InputState::new(window, cx).placeholder("e.g. meeting-notes");
            input.set_value(name, window, cx);
            input
        });
        let description = cx.new(|cx| {
            let mut input = TextareaState::new(window, cx)
                .auto_grow(2, 5)
                .placeholder("When should Aether use this? e.g. Use when the user pastes meeting notes or a transcript.");
            input.set_value(description, window, cx);
            input
        });
        let instructions = cx.new(|cx| {
            let mut input = TextareaState::new(window, cx)
                .auto_grow(8, 24)
                .placeholder("What to do once the skill is loaded. Markdown works.");
            input.set_value(instructions, window, cx);
            input
        });
        name.update(cx, |input, cx| input.focus(window, cx));
        self.skill_editor = Some(SkillEditor {
            original,
            name,
            description,
            instructions,
            error: None,
            confirm_delete: false,
            preview: false,
        });
        cx.notify();
    }

    fn save_skill(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &mut self.skill_editor else {
            return;
        };
        let value =
            |input: &Entity<TextareaState>, cx: &Context<Self>| input.read(cx).value().to_string();
        let result = skills::save(
            editor.original.as_ref(),
            &editor.name.read(cx).value(),
            &value(&editor.description, cx),
            &value(&editor.instructions, cx),
        );
        match result {
            Ok(saved) => {
                if let Some(original) = &editor.original
                    && original.name != saved.name
                {
                    let enabled = self.settings.skill_enabled(original);
                    let (old, new) = (original.id(), saved.id());
                    self.update_settings(cx, |s| {
                        s.skills.remove(&old);
                        s.skills.insert(new, enabled);
                    });
                }
                self.skills = skills::load_all();
                self.skill_editor = None;
                self.skill_tab = SkillSource::Aether;
            }
            Err(error) => editor.error = Some(error.into()),
        }
        cx.notify();
    }

    fn delete_skill(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &mut self.skill_editor else {
            return;
        };
        let Some(skill) = editor.original.clone() else {
            return;
        };
        if !editor.confirm_delete {
            editor.confirm_delete = true;
            return cx.notify();
        }
        match skills::delete(&skill) {
            Ok(()) => {
                let id = skill.id();
                self.update_settings(cx, |s| {
                    s.skills.remove(&id);
                });
                self.skills = skills::load_all();
                self.skill_editor = None;
            }
            Err(e) => editor.error = Some(format!("Couldn't delete: {e}").into()),
        }
        cx.notify();
    }

    fn open_skills_folder(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = skills::dir(SkillSource::Aether) else {
            return;
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("couldn't create {}: {e}", dir.display());
            return;
        }
        cx.open_with_system(&dir);
    }

    pub(crate) fn render_skills(&self, cx: &mut Context<Self>) -> SettingsPage {
        let page = SettingsPage::new("Skills")
            .on_back(cx.listener(|this, _, _, cx| this.open_page(Page::Chat, cx)))
            .action(
                aether_ui::button("open-skills-folder")
                    .ghost()
                    .small()
                    .icon(IconName::FolderOpen)
                    .label("Open folder")
                    .on_click(cx.listener(|this, _, _, cx| this.open_skills_folder(cx))),
            )
            .action(
                aether_ui::button("new-skill")
                    .primary()
                    .small()
                    .icon(IconName::Plus)
                    .label("New skill")
                    .on_click(cx.listener(|this, _, window, cx| this.edit_skill(None, window, cx))),
            )
            .child(div().text_sm().text_color(cx.theme().muted_foreground).child(
                "Skills teach Aether how to handle specific tasks. It loads one when a \
                         message needs it, or when you mention it as @name.",
            ));
        match &self.skill_editor {
            Some(editor) => page.child(self.render_skill_editor(editor, cx)),
            None => page.fill().child(self.render_skill_list(cx)),
        }
    }

    /// The current tab's skills that match the search, in display order.
    fn shown_skills(&self, cx: &App) -> Vec<Skill> {
        let query = self.skill_search.read(cx).value().to_lowercase();
        self.skills
            .iter()
            .filter(|s| s.source == self.skill_tab)
            .filter(|s| {
                query.is_empty()
                    || s.name.contains(&query)
                    || s.description.to_lowercase().contains(&query)
            })
            .cloned()
            .collect()
    }

    fn render_skill_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        let surface = if theme.is_dark() { theme.secondary.opacity(0.5) } else { theme.background };
        let tab = self.skill_tab;
        let shown = self.shown_skills(cx).len();
        let in_tab = self.skills.iter().filter(|s| s.source == tab).count();

        let tabs = SkillSource::ALL.into_iter().fold(
            Segmented::new("skill-source")
                .selected(SkillSource::ALL.iter().position(|s| *s == tab).unwrap_or(0)),
            |tabs, source| {
                let count = self.skills.iter().filter(|s| s.source == source).count();
                tabs.option(format!("{} {count}", source.label()))
            },
        );
        let title = if tab.editable() {
            "Your skills".to_string()
        } else {
            format!("From {} · read-only", tab.label())
        };
        let body = if shown == 0 {
            let message = if in_tab > 0 {
                "No skills match your search.".to_string()
            } else if tab.editable() {
                "No skills yet. Create one with New skill.".to_string()
            } else {
                let dir = skills::dir(tab).map(|d| d.display().to_string()).unwrap_or_default();
                format!("No skills found in {dir}.")
            };
            div().py_6().text_center().text_sm().text_color(muted).child(message).into_any_element()
        } else {
            v_flex()
                .flex_1()
                .min_h_0()
                .gap_2()
                .child(div().px_1().text_sm().text_color(muted).child(title))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .corner(rems(0.75))
                        .border_1()
                        .border_color(border)
                        .bg(surface)
                        .overflow_hidden()
                        .child(
                            uniform_list(
                                "skills",
                                shown,
                                cx.processor(|this, range: Range<usize>, _, cx| {
                                    let shown = this.shown_skills(cx);
                                    range
                                        .filter_map(|ix| Some((ix, shown.get(ix)?)))
                                        .map(|(ix, skill)| {
                                            this.render_skill_row(skill, ix == 0, cx)
                                        })
                                        .collect::<Vec<_>>()
                                }),
                            )
                            .size_full(),
                        ),
                )
                .into_any_element()
        };

        v_flex()
            .flex_1()
            .min_h_0()
            .gap_4()
            .child(
                h_flex()
                    .gap_3()
                    .justify_between()
                    .child(tabs.on_change(cx.listener(|this, ix: &usize, _, cx| {
                        this.skill_tab = SkillSource::ALL[*ix];
                        cx.notify();
                    })))
                    .child(
                        div()
                            .w(px(240.))
                            .child(field(&self.skill_search).small().icon(IconName::Search)),
                    ),
            )
            .child(body)
    }

    /// One fixed-height row, so the list can render only what's visible.
    fn render_skill_row(
        &self,
        skill: &Skill,
        first: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (muted, border) = (cx.theme().muted_foreground, cx.theme().border);
        let id = skill.id();
        let toggle = Switch::new(SharedString::from(format!("skill-{id}")))
            .checked(self.settings.skill_enabled(skill))
            .accessibility_label(format!("Use {}", skill.name))
            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                let (id, checked) = (id.clone(), *checked);
                this.update_settings(cx, |s| {
                    s.skills.insert(id, checked);
                });
            }));
        let edit = skill.source.editable().then(|| {
            let skill = skill.clone();
            aether_ui::button(SharedString::from(format!("edit-{}", skill.name)))
                .ghost()
                .small()
                .label("Edit")
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.edit_skill(Some(skill.clone()), window, cx)
                }))
        });
        h_flex()
            .w_full()
            .gap_4()
            .px_4()
            .py_3()
            .when(!first, |row| row.border_t_1().border_color(border))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(div().text_sm().font_medium().child(format!("@{}", skill.name)))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .truncate()
                            .child(skill.description.clone()),
                    ),
            )
            .child(h_flex().flex_none().gap_2().children(edit).child(toggle))
    }

    fn render_skill_editor(
        &self,
        editor: &SkillEditor,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let title = match &editor.original {
            Some(skill) => format!("Edit @{}", skill.name),
            None => "New skill".into(),
        };
        let delete = editor.original.is_some().then(|| {
            aether_ui::button("delete-skill")
                .ghost()
                .small()
                .label(if editor.confirm_delete { "Click again to delete" } else { "Delete" })
                .when(editor.confirm_delete, |b| b.text_color(aether_ui::theme::danger_text(cx)))
                .on_click(cx.listener(|this, _, _, cx| this.delete_skill(cx)))
        });
        SettingsSection::new(title)
            .child(
                SettingsRow::new("Name")
                    .description("Lowercase letters, numbers and hyphens. You mention it as @name.")
                    .stacked()
                    .control(field(&editor.name)),
            )
            .child(
                SettingsRow::new("Description")
                    .description("Aether reads this to decide when the skill applies.")
                    .stacked()
                    .control(field_area(&editor.description)),
            )
            .child(
                SettingsRow::new("Instructions")
                    .description("What to do once the skill is loaded.")
                    .stacked()
                    .control(
                        v_flex()
                            .gap_3()
                            .child(
                                aether_ui::button("skill-markdown-preview")
                                    .debug_selector(|| "skill-markdown-preview-toggle".into())
                                    .small()
                                    .ghost()
                                    .icon(if editor.preview {
                                        gpui_kit::assets::IconName::Pencil
                                    } else {
                                        gpui_kit::assets::IconName::Eye
                                    })
                                    .tooltip(if editor.preview {
                                        "Edit Markdown source"
                                    } else {
                                        "Preview Markdown"
                                    })
                                    .accessibility_label(if editor.preview {
                                        "Edit Markdown source"
                                    } else {
                                        "Preview Markdown"
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if let Some(editor) = &mut this.skill_editor {
                                            editor.preview = !editor.preview;
                                            if editor.preview {
                                                window.blur(cx);
                                            } else {
                                                editor.instructions.update(cx, |input, cx| {
                                                    input.focus(window, cx)
                                                });
                                            }
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(if editor.preview {
                                let view = cx.entity().downgrade();
                                div()
                                    .id("skill-markdown-preview-body")
                                    .debug_selector(|| "skill-markdown-preview-body".into())
                                    .min_h(px(120.))
                                    .child(
                                        Markdown::new(
                                            format!(
                                                "skill-instructions-{}",
                                                editor.instructions.entity_id()
                                            ),
                                            editor.instructions.read(cx).value(),
                                        )
                                        .on_reflow(Some(
                                            std::rc::Rc::new(move |cx| {
                                                view.update(cx, |_, cx| cx.notify()).ok();
                                            }),
                                        )),
                                    )
                                    .into_any_element()
                            } else {
                                field_area(&editor.instructions).into_any_element()
                            }),
                    ),
            )
            .child(
                h_flex()
                    .p_4()
                    .gap_2()
                    .children(delete)
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .text_color(aether_ui::theme::danger_text(cx))
                            .children(editor.error.clone()),
                    )
                    .child(
                        aether_ui::button("cancel-skill").ghost().small().label("Cancel").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.skill_editor = None;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        aether_ui::button("save-skill")
                            .primary()
                            .small()
                            .label("Save")
                            .on_click(cx.listener(|this, _, _, cx| this.save_skill(cx))),
                    ),
            )
    }
}

#[cfg(test)]
#[path = "../tests/unit/skills_view.rs"]
mod tests;
