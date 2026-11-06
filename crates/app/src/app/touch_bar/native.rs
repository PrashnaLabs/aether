//! AppKit ownership stays here. The window retains the bar, which retains both
//! the weak button targets and the task that drains their events on GPUI's UI loop.
use super::ButtonAction;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui_kit::{Task, Window};
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained, sel};
use objc2_app_kit::{
    NSButton, NSCellImagePosition, NSCustomTouchBarItem, NSImage, NSTouchBar, NSTouchBarItem,
    NSView,
};
use objc2_foundation::{MainThreadMarker, NSArray, NSObject, NSObjectProtocol, NSSet, NSString};
use std::sync::atomic::{AtomicBool, Ordering};
use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};

struct TargetIvars {
    action: ButtonAction,
    sender: UnboundedSender<ButtonAction>,
}

define_class!(
    // SAFETY: NSObject has no additional subclass requirements. All access is
    // main-thread-only and the selector has the standard target/action signature.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = TargetIvars]
    #[name = "AetherTouchBarTarget"]
    struct ActionTarget;

    unsafe impl NSObjectProtocol for ActionTarget {}

    impl ActionTarget {
        #[unsafe(method(performAction:))]
        fn perform_action(&self, _sender: &NSButton) {
            // Do not borrow GPUI from an AppKit callback. The receiver runs later.
            let _ = self.ivars().sender.unbounded_send(self.ivars().action);
        }
    }
);

struct BarIvars {
    buttons: Vec<Retained<NSButton>>,
    timer_button: Retained<NSButton>,
    base_identifiers: Vec<Retained<NSString>>,
    timer_identifier: Retained<NSString>,
    timer_visible: AtomicBool,
    _targets: Vec<Retained<ActionTarget>>,
    _task: Task<()>,
}

define_class!(
    // SAFETY: NSTouchBar has no additional subclass requirements. The native
    // window and all retained controls are accessed only on the main thread.
    #[unsafe(super(NSTouchBar, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = BarIvars]
    #[name = "AetherActionTouchBar"]
    struct ActionBar;

    unsafe impl NSObjectProtocol for ActionBar {}
);

impl ActionBar {
    fn new(
        mtm: MainThreadMarker,
        start: impl FnOnce(UnboundedReceiver<ButtonAction>) -> Task<()>,
    ) -> Retained<Self> {
        let (sender, receiver) = unbounded();
        let mut targets = Vec::new();
        let mut buttons = Vec::new();
        let mut items: Vec<Retained<NSTouchBarItem>> = Vec::new();
        let mut identifiers = Vec::new();
        for (action, label, name, symbol) in [
            (ButtonAction::NewChat, "New Chat", "new-chat", "plus"),
            (ButtonAction::Terminal, "Terminal", "terminal", "terminal"),
            (ButtonAction::Files, "Files", "files", "folder"),
            (ButtonAction::Changes, "Git Changes", "changes", "arrow.triangle.branch"),
            (ButtonAction::Commands, "Commands", "commands", "command"),
            (ButtonAction::Settings, "Settings", "settings", "gearshape"),
        ] {
            let target =
                ActionTarget::alloc(mtm).set_ivars(TargetIvars { action, sender: sender.clone() });
            // SAFETY: NSObject's initializer has this signature and the target
            // implements performAction: with the sender type used by NSButton.
            let target: Retained<ActionTarget> = unsafe { msg_send![super(target), init] };
            let button = unsafe {
                NSButton::buttonWithTitle_target_action(
                    &NSString::from_str(label),
                    Some(&target),
                    Some(sel!(performAction:)),
                    mtm,
                )
            };
            // Keep the title usable if a symbol is unavailable on an older macOS.
            if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str(symbol),
                Some(&NSString::from_str(label)),
            ) {
                button.setImage(Some(&image));
                button.setImagePosition(
                    if matches!(
                        action,
                        ButtonAction::Terminal
                            | ButtonAction::Files
                            | ButtonAction::Changes
                            | ButtonAction::Commands
                            | ButtonAction::Settings
                    ) {
                        NSCellImagePosition::ImageOnly
                    } else {
                        NSCellImagePosition::ImageLeading
                    },
                );
            }
            let identifier = NSString::from_str(&format!("dev.aether.touchbar.{name}"));
            let item = NSCustomTouchBarItem::initWithIdentifier(
                NSCustomTouchBarItem::alloc(mtm),
                &identifier,
            );
            item.setView(&button);
            item.setCustomizationLabel(Some(&NSString::from_str(label)));
            identifiers.push(identifier);
            items.push(item.into_super());
            targets.push(target);
            buttons.push(button);
        }
        let timer_button = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(""),
                None,
                None,
                mtm,
            )
        };
        timer_button.setBordered(false);
        if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str("stopwatch"),
            Some(&NSString::from_str("Running time")),
        ) {
            timer_button.setImage(Some(&image));
            timer_button.setImagePosition(NSCellImagePosition::ImageLeading);
        }
        let timer_identifier = NSString::from_str("dev.aether.touchbar.timer");
        let timer_item = NSCustomTouchBarItem::initWithIdentifier(
            NSCustomTouchBarItem::alloc(mtm),
            &timer_identifier,
        );
        timer_item.setView(&timer_button);
        timer_item.setCustomizationLabel(Some(&NSString::from_str("Running time")));
        items.push(timer_item.into_super());

        let this = Self::alloc(mtm).set_ivars(BarIvars {
            buttons,
            timer_button,
            base_identifiers: identifiers.clone(),
            timer_identifier,
            timer_visible: AtomicBool::new(false),
            _targets: targets,
            _task: start(receiver),
        });
        // SAFETY: NSTouchBar's init returns an initialized instance of this subclass.
        let bar: Retained<Self> = unsafe { msg_send![super(this), init] };
        bar.setTemplateItems(&NSSet::from_retained_slice(&items));
        bar.setDefaultItemIdentifiers(&NSArray::from_retained_slice(&identifiers));
        bar
    }

    fn update(&self, enabled: [bool; 6], elapsed: Option<String>) {
        for (button, enabled) in self.ivars().buttons.iter().zip(enabled) {
            if button.isEnabled() != enabled {
                button.setEnabled(enabled);
            }
        }
        let ivars = self.ivars();
        let is_running = elapsed.is_some();
        if let Some(time_str) = elapsed {
            let title = NSString::from_str(&time_str);
            ivars.timer_button.setTitle(&title);
        }
        let was_running = ivars.timer_visible.swap(is_running, Ordering::Relaxed);
        if was_running != is_running {
            if is_running {
                let mut active = ivars.base_identifiers.clone();
                active.push(ivars.timer_identifier.clone());
                self.setDefaultItemIdentifiers(&NSArray::from_retained_slice(&active));
            } else {
                self.setDefaultItemIdentifiers(&NSArray::from_retained_slice(&ivars.base_identifiers));
            }
        }
    }
}

pub(super) fn sync(
    window: &Window,
    enabled: [bool; 6],
    elapsed: Option<String>,
    start: impl FnOnce(UnboundedReceiver<ButtonAction>) -> Task<()>,
) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Ok(handle) = HasWindowHandle::window_handle(window) else { return };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return };
    // SAFETY: GPUI supplies this NSView for a live window. The Window borrow
    // keeps it alive for this call, and MainThreadMarker checks thread affinity.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    let Some(native) = view.window() else { return };
    if let Some(bar) = native.touchBar()
        && let Some(bar) = bar.downcast_ref::<ActionBar>()
    {
        bar.update(enabled, elapsed);
        return;
    }
    let bar = ActionBar::new(mtm, start);
    bar.update(enabled, elapsed);
    native.setTouchBar(Some(&bar));
}
