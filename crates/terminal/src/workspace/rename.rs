use aether_ui::InlineRename;
use gpui_kit::{AnyElement, Context, IntoElement, SharedString, Window};

use super::{GroupId, PaneId, RenameTarget, TerminalWorkspace};

impl TerminalWorkspace {
    pub(super) fn begin_rename_group(
        &mut self,
        group_id: GroupId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let initial = self.group_display_label(group_id, cx).to_string();
        self.renaming = Some(RenameTarget::Group(group_id));
        self.rename_input.update(cx, |input, cx| {
            input.set_value(initial, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    pub(super) fn begin_rename_pane(
        &mut self,
        pane_id: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let initial = self.pane_display_label(pane_id, cx).to_string();
        self.renaming = Some(RenameTarget::Pane(pane_id));
        self.rename_input.update(cx, |input, cx| {
            input.set_value(initial, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    pub(super) fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.renaming.take() else {
            return;
        };
        let value = self.rename_input.read(cx).value().trim().to_string();
        let custom = (!value.is_empty()).then(|| SharedString::from(value));
        match target {
            RenameTarget::Group(group_id) => {
                self.model.set_group_title(group_id, custom.clone());
                if let Some(group) = self.model.group(group_id)
                    && group.pane_count() == 1
                    && let Some(pane) = self.panes.get(&group.active_pane)
                {
                    pane.update(cx, |p, cx| p.set_custom_title(custom, cx));
                }
            }
            RenameTarget::Pane(pane_id) => {
                if let Some(pane) = self.panes.get(&pane_id) {
                    pane.update(cx, |p, cx| p.set_custom_title(custom.clone(), cx));
                }
                if let Some(group_id) = self.model.group_of_pane(pane_id)
                    && let Some(group) = self.model.group(group_id)
                    && group.pane_count() == 1
                {
                    self.model.set_group_title(group_id, custom);
                }
            }
        }
        cx.notify();
    }

    fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.renaming.take().is_some() {
            self.focus_active_pane(window, cx);
            cx.notify();
        }
    }

    pub(super) fn render_sidebar_rename_row(
        &self,
        title: &SharedString,
        cx: &Context<Self>,
    ) -> AnyElement {
        InlineRename::row(title.clone(), &self.rename_input)
            .on_cancel(cx.listener(|this, _, window, cx| this.cancel_rename(window, cx)))
            .into_any_element()
    }

    pub(super) fn render_tab_rename_row(
        &self,
        title: &SharedString,
        cx: &Context<Self>,
    ) -> AnyElement {
        InlineRename::tab(title.clone(), &self.rename_input)
            .on_cancel(cx.listener(|this, _, window, cx| this.cancel_rename(window, cx)))
            .into_any_element()
    }
}
