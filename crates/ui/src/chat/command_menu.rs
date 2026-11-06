use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme, IconName, IndexPath,
    command::{Command, CommandGroup, CommandState},
    input::InputState,
};
use gpui_kit::{App, Entity, MouseButton, Window, black, div, prelude::*, px};

use crate::components::field;

type OnConfirm = Rc<dyn Fn(IndexPath, &mut Window, &mut App)>;
type OnDismiss = Rc<dyn Fn(&mut Window, &mut App)>;

/// VS Code style command palette: a dimmed backdrop with a searchable list
/// of command groups near the top of the window. The caller owns `query` and
/// passes only the groups that match it.
#[derive(IntoElement)]
pub struct CommandMenu {
    state: Entity<CommandState>,
    query: Entity<InputState>,
    groups: Vec<CommandGroup>,
    on_confirm: Option<OnConfirm>,
    on_dismiss: Option<OnDismiss>,
}

impl CommandMenu {
    pub fn new(state: &Entity<CommandState>, query: &Entity<InputState>) -> Self {
        Self {
            state: state.clone(),
            query: query.clone(),
            groups: Vec::new(),
            on_confirm: None,
            on_dismiss: None,
        }
    }

    pub fn group(mut self, group: CommandGroup) -> Self {
        self.groups.push(group);
        self
    }

    /// Called with the confirmed item's group and row.
    pub fn on_confirm(
        mut self,
        handler: impl Fn(IndexPath, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_confirm = Some(Rc::new(handler));
        self
    }

    /// Called on Escape with an empty query or a click outside the menu.
    pub fn on_dismiss(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for CommandMenu {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let search = self.query.clone();
        let command = self.groups.into_iter().fold(
            Command::new(&self.state)
                .searchable(false)
                .header(move |_, _, _| {
                    div()
                        .p_2()
                        .child(field(&search).icon(IconName::Search).aria_label("Search commands"))
                })
                .max_h(px(420.)),
            Command::group,
        );
        let command = match self.on_confirm {
            Some(f) => command.on_confirm(move |ix, window, cx| f(ix, window, cx)),
            None => command,
        };
        let query = self.query;
        let dismiss = self.on_dismiss.clone();
        let command = command.on_cancel(move |window, cx| {
            if !query.read(cx).value().is_empty() {
                query.update(cx, |q, cx| q.clean(window, cx));
            } else if let Some(f) = &dismiss {
                f(window, cx);
            }
        });
        div()
            .id("command-menu")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_start()
            .justify_center()
            .pt(px(96.))
            .bg(black().opacity(0.35))
            .when_some(self.on_dismiss, |backdrop, f| {
                backdrop.on_mouse_down(MouseButton::Left, move |_, window, cx| f(window, cx))
            })
            .child(
                div()
                    .w(px(560.))
                    .h_auto()
                    .rounded(theme.radius_lg)
                    .shadow_2xl()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(command),
            )
    }
}
