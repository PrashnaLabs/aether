use super::*;
use aether_ui::field;
use gpui_kit::component::{
    ActiveTheme, Disableable, Sizable, StyledExt,
    button::ButtonVariants,
    h_flex,
    menu::{DropdownMenu, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{AnyElement, div, px};

impl ChatApp {
    pub(crate) fn render_notes(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let busy = self.notes.busy;
        let mut page =
            v_flex().id("notes-page").debug_selector(|| "notes-page".into()).size_full().min_h_0();
        if let Some(form) = &self.notes.form {
            let title = match form {
                Form::Create(_) => "New note",
                Form::Capture => "Save to note",
                Form::Rename(_) => "Rename / move",
                Form::ArchiveAge => "Archive notes older than days",
                Form::EmptyTrash => "Permanently delete all trashed notes?",
            };
            page = page.child(
                v_flex()
                    .p_3()
                    .gap_2()
                    .border_b_1()
                    .border_color(border)
                    .child(div().font_medium().child(title))
                    .when(!busy && !matches!(form, Form::EmptyTrash), |el| {
                        el.child(field(&self.notes.name).aria_label("Note name or days"))
                    })
                    .when(!busy && matches!(form, Form::Create(NoteKind::Docs)), |el| {
                        el.child(field(&self.notes.template).aria_label("Template name"))
                    })
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                aether_ui::button("notes-submit")
                                    .debug_selector(|| "notes-submit".into())
                                    .small()
                                    .label(if matches!(form, Form::EmptyTrash) {
                                        "Delete permanently"
                                    } else {
                                        "Save"
                                    })
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.submit_note_form(window, cx)
                                    })),
                            )
                            .child(
                                aether_ui::button("notes-cancel")
                                    .small()
                                    .ghost()
                                    .label("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.notes.form = None;
                                        this.notes.initial.clear();
                                        cx.notify();
                                    })),
                            ),
                    ),
            );
        }
        let explorer = self.notes_explorer(cx);
        let root = explorer.read(cx).root.clone();
        let scope = self
            .notes
            .project
            .as_ref()
            .map(|project| root.join("projects").join(project))
            .unwrap_or_else(|| root.clone());
        let tree_root = match self.notes.listing {
            Listing::Templates => root.join(".templates"),
            Listing::Trash => root.join(".trash"),
            Listing::Archive => self
                .notes
                .project
                .as_ref()
                .map(|p| root.join("archive/projects").join(p))
                .unwrap_or_else(|| root.join("archive")),
            Listing::All => scope,
        };
        if explorer.read(cx).tree_state.root != tree_root {
            explorer.update(cx, |explorer, _| {
                explorer.tree_state = aether_explorer::FileTreeState::new(tree_root)
            });
        }
        let lights = aether_ui::traffic_lights_gap(self.settings.sidebar_collapsed, window, cx);
        let mut sidebar = v_flex()
            .w(px(270.))
            .h_full()
            .min_h_0()
            .flex_none()
            .border_r_1()
            .border_color(border)
            .bg(cx.theme().sidebar)
            .child(
                h_flex()
                    .h(px(40.))
                    .flex_none()
                    .px_3()
                    .gap_1()
                    .border_b_1()
                    .border_color(border)
                    .child(lights)
                    .child(div().text_xs().font_semibold().text_color(muted).child("NOTES"))
                    .child(
                        aether_ui::button("notes-scope-toggle")
                            .xsmall()
                            .ghost()
                            .disabled(busy)
                            .label(self.notes.project.clone().unwrap_or_else(|| "Global".into()))
                            .tooltip("Switch global / current project notes")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.notes.project = if this.notes.project.is_some() {
                                    None
                                } else {
                                    this.active_project_folder()
                                        .file_name()
                                        .map(|s| s.to_string_lossy().to_string())
                                };
                                this.notes.listing = Listing::All;
                                this.refresh_notes(cx);
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        aether_ui::button("notes-search")
                            .xsmall()
                            .ghost()
                            .icon(gpui_kit::component::IconName::Search)
                            .tooltip("Search notes (Cmd/Ctrl+Shift+N)")
                            .accessibility_label("Search all notes")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_notes_search(window, cx)
                            })),
                    )
                    .child(
                        aether_ui::button("notes-refresh")
                            .xsmall()
                            .ghost()
                            .icon(gpui_kit::component::IconName::RefreshCw)
                            .tooltip("Refresh notes")
                            .accessibility_label("Refresh notes")
                            .on_click(cx.listener(|this, _, window, cx| {
                                let explorer = this.notes_explorer(cx);
                                let root = explorer.read(cx).root.clone();
                                this.check_explorer_disk(&root, window, cx);
                                this.refresh_explorer(&explorer, cx);
                                this.refresh_notes(cx);
                            })),
                    )
                    .child(
                        aether_ui::button("notes-more")
                            .xsmall()
                            .ghost()
                            .disabled(busy)
                            .icon(gpui_kit::component::IconName::Ellipsis)
                            .tooltip("Notes actions")
                            .accessibility_label("Notes actions")
                            .dropdown_menu({
                                let view = cx.entity();
                                move |menu, _, _| notes_menu(menu, view.clone())
                            }),
                    ),
            )
            .child(
                v_flex()
                    .id("notes-folder-tree")
                    .debug_selector(|| "notes-folder-tree".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.render_explorer_tree_pane(window, cx)),
            );
        if let Some(error) = &self.notes.error {
            sidebar = sidebar.child(
                div()
                    .id("notes-error")
                    .flex_none()
                    .p_2()
                    .max_h(px(80.))
                    .overflow_y_scroll()
                    .text_xs()
                    .text_color(aether_ui::theme::danger_text(cx))
                    .child(error.clone()),
            );
        } else if busy {
            sidebar = sidebar.child(
                div().flex_none().px_3().py_1().text_xs().text_color(muted).child("Working…"),
            );
        }
        let editor = self.render_explorer_editor_canvas(window, cx);
        page.child(
            h_flex().flex_1().min_h_0().w_full().overflow_hidden().child(sidebar).child(editor),
        )
        .into_any_element()
    }
}
fn notes_menu(menu: PopupMenu, view: Entity<ChatApp>) -> PopupMenu {
    let new = view.clone();
    let daily = view.clone();
    let scratch = view.clone();
    let all = view.clone();
    let mut menu = menu
        .item(PopupMenuItem::new("New note…").on_click(move |_, window, cx| {
            new.update(cx, |this, cx| {
                this.notes_form(Form::Create(NoteKind::Named), String::new(), window, cx)
            })
        }))
        .item(PopupMenuItem::new("Daily note").on_click(move |_, window, cx| {
            daily.update(cx, |this, cx| {
                this.note_job(window, cx, |store| {
                    store.create(NoteKind::Daily, "", None, "").map(NoteResult::Open)
                })
            })
        }))
        .item(PopupMenuItem::new("Scratchpad").on_click(move |_, window, cx| {
            scratch.update(cx, |this, cx| {
                this.note_job(window, cx, |store| {
                    store.create(NoteKind::Scratchpad, "", None, "").map(NoteResult::Open)
                })
            })
        }))
        .separator()
        .item(PopupMenuItem::new("Notes folder").on_click(move |_, _, cx| {
            all.update(cx, |this, cx| {
                this.notes.listing = Listing::All;
                this.refresh_notes(cx);
            })
        }))
        .separator();
    for (title, kind) in [
        ("New document…", NoteKind::Docs),
        ("Timestamped note", NoteKind::Timestamped),
        ("Credential note…", NoteKind::Credentials),
        ("Password note…", NoteKind::Password),
    ] {
        let view = view.clone();
        menu = menu.item(PopupMenuItem::new(title).on_click(move |_, window, cx| {
            view.update(cx, |this, cx| {
                if kind == NoteKind::Timestamped {
                    this.note_job(window, cx, |store| {
                        store.create(NoteKind::Timestamped, "", None, "").map(NoteResult::Open)
                    });
                } else {
                    this.notes_form(Form::Create(kind), String::new(), window, cx);
                }
            })
        }));
    }
    let last = view.clone();
    menu = menu.separator().item(PopupMenuItem::new("Last note").on_click(move |_, window, cx| {
        last.update(cx, |this, cx| {
            this.note_job(window, cx, |store| store.last().map(NoteResult::Open))
        })
    }));
    for (title, listing) in [
        ("Templates", Listing::Templates),
        ("Archive", Listing::Archive),
        ("Trash", Listing::Trash),
    ] {
        let view = view.clone();
        menu = menu.item(PopupMenuItem::new(title).on_click(move |_, _, cx| {
            view.update(cx, |this, cx| {
                this.notes.listing = listing;
                this.refresh_notes(cx);
            })
        }));
    }
    let older = view.clone();
    let empty = view.clone();
    let sync = view.clone();
    let trash = view.clone();
    menu.separator()
        .item(PopupMenuItem::new("Archive older notes…").on_click(move |_, window, cx| {
            older
                .update(cx, |this, cx| this.notes_form(Form::ArchiveAge, String::new(), window, cx))
        }))
        .item(PopupMenuItem::new("Trash empty notes").on_click(move |_, window, cx| {
            empty.update(cx, |this, cx| {
                if this
                    .notes
                    .explorer
                    .as_ref()
                    .is_some_and(|exp| exp.read(cx).tabs.iter().any(|b| b.is_dirty))
                {
                    this.notes.error = Some("Save unsaved notes before cleanup.".into());
                    cx.notify();
                    return;
                }
                this.note_job(window, cx, |store| {
                    store
                        .clean_empty()
                        .map(|n| NoteResult::Message(format!("Trashed {n} empty notes")))
                });
            })
        }))
        .item(PopupMenuItem::new("Empty trash…").on_click(move |_, window, cx| {
            trash
                .update(cx, |this, cx| this.notes_form(Form::EmptyTrash, String::new(), window, cx))
        }))
        .item(PopupMenuItem::new("Sync notes with Git").on_click(move |_, window, cx| {
            sync.update(cx, |this, cx| {
                this.note_job(window, cx, |store| store.sync().map(NoteResult::Message))
            })
        }))
}
pub(crate) fn note_context(
    menu: PopupMenu,
    view: Entity<ChatApp>,
    path: PathBuf,
    listing: Listing,
) -> PopupMenu {
    if listing == Listing::Trash {
        return menu.item(PopupMenuItem::new("Restore").on_click(move |_, window, cx| {
            view.update(cx, |this, cx| {
                let path = path.clone();
                this.note_job(window, cx, move |store| store.restore(&path).map(NoteResult::Open));
            })
        }));
    }
    let mut menu = menu;
    for (title, editor) in
        [("Open in Aether", NotesEditor::Aether), ("Open in Neovim / tmux", NotesEditor::Neovim)]
    {
        let (view, path) = (view.clone(), path.clone());
        menu = menu.item(PopupMenuItem::new(title).on_click(move |_, window, cx| {
            view.update(cx, |this, cx| this.open_note(path.clone(), Some(editor), window, cx))
        }));
    }
    let (rename, rename_path) = (view.clone(), path.clone());
    menu = menu.separator().item(PopupMenuItem::new("Rename / move…").on_click(
        move |_, window, cx| {
            rename.update(cx, |this, cx| {
                this.notes_form(Form::Rename(rename_path.clone()), String::new(), window, cx)
            })
        },
    ));
    for (title, archive) in [("Archive", true), ("Move to trash", false)] {
        let (view, path) = (view.clone(), path.clone());
        menu = menu.item(PopupMenuItem::new(title).on_click(move |_, window, cx| {
            view.update(cx, |this, cx| {
                if !this.note_clean(&path, cx) {
                    return;
                }
                let path = path.clone();
                this.note_job(window, cx, move |store| {
                    if archive { store.archive(&path) } else { store.trash(&path) }.map(|_| {
                        NoteResult::Message(
                            if archive { "Note archived" } else { "Note moved to trash" }.into(),
                        )
                    })
                });
            })
        }));
    }
    let copy = path.clone();
    menu.separator()
        .item(PopupMenuItem::new("Copy path").on_click(move |_, _, cx| {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(copy.display().to_string()))
        }))
        .item(
            PopupMenuItem::new("Reveal in file manager")
                .on_click(move |_, _, cx| cx.reveal_path(&path)),
        )
}
