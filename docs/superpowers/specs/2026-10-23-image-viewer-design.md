# Built-in image viewer

Status: approved and implemented.

## Intent

View local images inside Aether. The user chose a built-in viewer rather than
macOS Quick Look. Opening an image must reach the viewer through existing UI,
without launching another application or treating binary image data as text.

## Approach

Use a native GPUI overlay over the window where the image was opened. This fits
Aether's rendering and input handling and leaves the current workspace intact.
An Explorer-only image tab would miss chat attachments. Quick Look would depend
on macOS and was not the user's chosen approach. The overlay is the builder's
recommended presentation, not an explicit user layout requirement.

## First version

- Fit the image to the available area on open, preserving its aspect ratio.
- Provide zoom in/out, actual size, fit-to-window, and dragging to pan when zoomed.
- Show the filename, dimensions, and zoom level with a close button.
- Show transparency against a subtle checkerboard background.
- Escape closes the viewer and restores previous keyboard focus. Viewer input
  must not reach chat shortcuts, including Stop generating.
- Keep viewer state local to its host window, including detached session windows.
- Use existing image decoders for PNG, JPEG, WebP, GIF, TIFF, and BMP. The first
  version displays a static frame for animated images. No remote-image fetching.
- Load and decode away from the UI thread, with bounded image dimensions and
  decoded-memory limits. Show loading, missing-file, and decode-error states.
- Preserve the source file. No editing, cropping, annotation, or export tools.

## Wiring

Explorer tree, Quick Open, and local image links route to the viewer before text
editor opening. Clicking images in transcript attachments and pending composer
attachments opens the same viewer. Attachment removal remains independent.
Ordinary text files retain their current opening behavior. Local image links
also work in direct chats, where agent Explorer routing currently does not.

## Verification

Check image detection, zoom/fit bounds, loading failures, and asynchronous result
ordering. Exercise Explorer and attachment/link routes, Escape focus restoration,
and isolation between main and detached windows. Compile the app and run the
relevant existing tests. Verify the rendered overlay with a real local image.
