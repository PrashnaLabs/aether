# Touch Bar action buttons

Status: approved and implemented.

## Intent

Give users of MacBook Pro Touch Bars direct access to common Aether actions.
The user requested action buttons. The button selection below is a proposal.

## Approach

Use native AppKit `NSTouchBar` controls attached to Aether's windows, with
macOS-only Rust integration. Route button taps through the existing app and
session actions so they use the same behavior as the keyboard and menus.

A third-party Touch Bar utility would require users to install and configure
another app. Keyboard emulation would depend on key bindings and focus. Native
controls are the recommended approach for this native GPUI application.

## Buttons and behavior

- New Chat: create a chat in the main workspace using its current project and
  the existing New Chat behavior. From a detached window, activate the main window.
- Terminal: toggle the terminal for the current session.
- Files: open the existing file explorer.
- Git Changes: toggle the existing changes panel.
- Commands: open the command menu.

Provide concise labels and native icons where suitable. Keep macOS's system
Control Strip available. Buttons must work from the main window and detached
session windows, with session actions targeting the focused pane. Actions that
are unavailable in a window or context must be disabled or omitted.

## Constraints and scope

Compile the native integration only on macOS. Machines without a Touch Bar
must retain their existing behavior. Preserve the existing Touch Bar-related
window lifecycle workaround and unrelated working-tree changes.

Do not add Touch Bar settings, automatic message submission, approval buttons,
or custom drawing in this first version. The implementer chooses native object
ownership and callback details after checking the GPUI and AppKit APIs.

## Validation

Test button-to-action mapping and context-dependent availability. Run formatting,
the relevant app tests, and a macOS compile check. Check native object and window
lifetimes during implementation. Actual display, taps, and focus changes need
a Touch Bar Mac or Apple's Touch Bar simulator; report any checks not performed.

## Follow-up changes

The user requested native icons beside the action labels, then removal of Stop
from the Touch Bar to prevent accidental cancellation. The bar now contains five
actions: New Chat, Terminal, Files, Git Changes, and Commands.

Terminal, Files, and Git Changes now show icons only, retaining their descriptive native
titles for accessibility and customization. Settings was added with a gear icon
and label, opening the main window’s Settings page from either window. Stop
remains excluded.

Settings and Commands now also show icons only, matching Terminal, Files, and
Git Changes, while retaining their labels for accessibility and customization.
An elapsed run timer item is added to the Touch Bar with a native stopwatch icon,
dynamically visible only while an agent turn is actively running for the focused
session in that window, and automatically hiding when idle.
