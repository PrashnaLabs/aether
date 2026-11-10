mod anchored;
mod branch_select;
mod brand_mark;
mod choice_cards;
mod choice_menu;
mod column;
mod composer_chip;
mod field;
mod file_icon;
mod glow_card;
mod provider_icon;
mod segmented;
mod tabs;
mod workspace_select;

pub use anchored::{TriggerBounds, anchored_popup, popup_height, room};
pub use branch_select::{
    BRANCH_SELECT_CONTEXT, BranchSelect, BranchSelectEvent, BranchSelectState, SelectConfirm,
    SelectDismiss, SelectNext, SelectPrev,
};
pub use brand_mark::BrandMark;
pub use choice_cards::{CAROUSEL_CARD_WIDTH, CardCarousel, CardRow, ChoiceCard};
pub use choice_menu::{
    CHOICE_MENU_CONTEXT, ChoiceConfirm, ChoiceDismiss, ChoiceDown, ChoiceFirst, ChoiceLast,
    ChoiceMenu, ChoiceMenuEvent, ChoiceMenuState, ChoiceOption, ChoiceUp,
};
pub use column::Column;
pub use composer_chip::{composer_chip, control_chip, icon_chip, overlay, overlay_strong};
pub use field::{field, field_area, field_fill};
pub use file_icon::{file_icon, file_icon_color};
pub use glow_card::GlowCard;
pub use provider_icon::{provider_icon, provider_icon_path, provider_tint};
pub use segmented::Segmented;
pub use tabs::{InlineRename, Tab, TabBar, window_controls_gap};
pub use workspace_select::{
    WorkspaceSelect, WorkspaceSelectEvent, WorkspaceSelectState, WorkspaceSelection, WorktreeRow,
};

use gpui_kit::component::button::Button;
use gpui_kit::{App, ClickEvent, ElementId, Styled, Window};

/// A `Button` with the hand cursor; gpui-component keeps the arrow on most variants.
pub fn button(id: impl Into<ElementId>) -> Button {
    Button::new(id).cursor_pointer()
}

pub type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
