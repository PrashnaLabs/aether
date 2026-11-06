# Image viewer implementation plan

> Execute inline with superpowers:executing-plans. Preserve unrelated working-tree changes.

**Goal:** View local images inside Aether from Explorer, attachments, and reply links.
**Architecture:** A GPUI viewer entity per native window loads bounded image data in the background. App roots render the viewer overlay; common image-opening hooks route to it before text-file handling.
**Tech stack:** Rust, GPUI, existing image decoders.
**Spec:** docs/superpowers/specs/2026-10-23-image-viewer-design.md

## Constraints and review focus

Keep source files unchanged and existing text-file behavior. Support PNG/JPEG/WebP/GIF/TIFF/BMP, with static animation frames. Isolate windows and restore focus. Consume viewer shortcuts before chat actions. Limit decoding memory and dimensions; ignore stale asynchronous loads.

## Tasks

- [x] Add focused failing tests for detection, fit/zoom/pan math, and bounded decoding failures.
- [x] Implement viewer entity in crates/app/src/image_viewer.rs and image_viewer/ modules, with fit, actual size, zoom, pan, checkerboard, filename/dimensions, loading/errors, and close/focus restoration.
- [x] Add per-window viewer ownership and overlay rendering to main and detached roots.
- [x] Route Explorer/Quick Open, local image links, transcript attachments and pending attachment clicks to the viewer.
- [x] Verify routing and viewer input isolation with GPUI tests. Run app/UI tests and compile checks.
- [x] Inspect a rendered local image and record verification and limits.

## Verification record

- Initial detection, fit, and pan regression tests failed before implementation.
- Eight viewer tests cover supported decoders and static GIF frames; corrupt,
  missing, and oversized files; BGRA/alpha correctness; stale loading/close;
  Explorer and direct-chat local-link routing; detached-window isolation; and
  Escape restoring focus without stopping generation.
- Independent review caught chat shortcuts dispatching before element key-down
  handlers. A `/` then Escape regression reproduced the focus loss. A retained,
  window-scoped keystroke interceptor fixes it, with regression coverage for
  `/`, Cmd+N/K/P, Escape, and main-window shortcuts while a detached viewer opens.
- Review confirmed the fix and reported no remaining blocking findings.
- `cargo check --locked -p aether`: passed.
- Final focused viewer run: 8 passed, including folder names with image suffixes
  preserving normal directory routing.
- `cargo test --locked --workspace`: passed, 1426 tests, 8 ignored.
- Changed Rust files pass formatting with child traversal disabled; unrelated
  existing formatting in sidebar/editor/git-panel code is preserved.
- A temporary application using the production GPUI viewer loaded a real
  1600×1000 translucent PNG and rendered the toolbar, fit view, and checkerboard.
  Native window capture succeeded: `/tmp/aether-image-viewer-preview.png`.
  Temporary smoke-example files were removed afterward.
- The existing `block v0.1.6` future-compatibility warning remains.

## Implementation decisions

Viewer ownership is per native window, independent of session UI swapping.
Background loading limits each file to 64 MiB, 32 megapixels, 16,384 pixels per
side, and 128 MiB decoder allocations. Full animation and remote URLs stay out
of scope. Non-image attachments retain system-app opening through GPUI's
cross-platform `open_with_system` API. File-link routing is initialized even
when background services are disabled, since it is UI behavior.
