use super::data::*;
use std::path::Path;

#[test]
fn raster_images_route_to_viewer_but_source_files_do_not() {
    for name in ["photo.PNG", "a.jpg", "a.jpeg", "a.webp", "a.gif", "a.tiff", "a.tif", "a.bmp"] {
        assert!(is_image(Path::new(name)), "{name}");
    }
    for name in ["main.rs", "readme.md", "a.svg", "image.png.rs", "file"] {
        assert!(!is_image(Path::new(name)), "{name}");
    }
}

#[test]
fn fit_preserves_aspect_ratio_without_enlarging_small_images() {
    assert_eq!(fit_scale([2000., 1000.], [800., 600.]), 0.4);
    assert_eq!(fit_scale([1000., 2000.], [800., 600.]), 0.3);
    assert_eq!(fit_scale([100., 100.], [800., 600.]), 1.);
    assert!(fit_scale([0., 0.], [0., 0.]).is_finite());
}

#[test]
fn pan_cannot_lose_the_image_outside_the_viewport() {
    assert_eq!(clamp_pan([900., -800.], [1200., 1000.], [800., 600.]), [200., -200.]);
    assert_eq!(clamp_pan([50., 40.], [100., 100.], [800., 600.]), [0., 0.]);
}

fn test_directory() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "aether-image-tests-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn decoder_preserves_dimensions_alpha_and_bgra_colors() {
    let dir = test_directory();
    let path = dir.join("red.png");
    image::RgbaImage::from_pixel(2, 1, image::Rgba([255, 20, 10, 128])).save(&path).unwrap();
    let loaded = load(&path).unwrap();
    assert_eq!(loaded.dimensions, [2, 1]);
    assert_eq!(loaded.pixels.as_bytes(0).unwrap(), &[10, 20, 255, 128, 10, 20, 255, 128]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn decoder_rejects_missing_corrupt_and_oversized_images() {
    let dir = test_directory();
    assert!(load(&dir.join("missing.png")).is_err());
    let path = dir.join("bad.png");
    std::fs::write(&path, b"not an image").unwrap();
    assert!(load(&path).is_err());
    let large = dir.join("large.png");
    image::RgbaImage::new(16_385, 1).save(&large).unwrap();
    assert!(load(&large).is_err());
    let file = std::fs::File::create(&large).unwrap();
    file.set_len(65 * 1024 * 1024).unwrap();
    assert!(load(&large).err().unwrap().contains("64 MB"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[gpui_kit::test]
fn image_viewer_routes_local_images_and_escape_restores_focus_without_stopping(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::app::ChatApp;
    use aether_agent::{Agent, Cancel};
    use aether_core::Settings;
    use gpui_kit::{AppContext, VisualTestContext};

    cx.background_executor.allow_parking();
    cx.update(gpui_kit::init);
    cx.update(crate::actions::bind_keys);
    let dir = test_directory();
    let path = dir.join("test.png");
    image::RgbaImage::from_pixel(80, 40, image::Rgba([255, 0, 0, 255])).save(&path).unwrap();
    let (window, app) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |window, cx| {
            cx.new(|cx| {
                ChatApp::with_state(
                    Agent::new().unwrap(),
                    Settings::default(),
                    None,
                    None,
                    false,
                    window,
                    cx,
                )
            })
        })
        .unwrap()
    });
    let (id, previous) = cx
        .update_window(window, |_, window, cx| {
            app.update(cx, |app, cx| {
                app.focus_composer(window, cx);
                app.chats[app.active].cancel = Some(Cancel::inert());
                let previous = window.focused(cx).unwrap();
                // An image suffix on a directory must preserve folder opening.
                let folder = dir.join("folder.png");
                std::fs::create_dir_all(&folder).unwrap();
                assert!(!app.preview_image(&folder, window, cx));
                // Explorer/Quick Open must intercept images before creating a text buffer.
                app.open_explorer_file(path.clone(), window, cx);
                let id = window.window_handle().window_id();
                assert!(app.image_viewers[&id].0.read(cx).visible);
                assert!(app.image_viewer_overlay(window, cx).is_some());
                (id, previous)
            })
        })
        .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let viewer = app.read(cx).image_viewers[&id].0.read(cx);
        assert_eq!(viewer.loaded.as_ref().unwrap().dimensions, [80, 40]);
    });
    let mut visual = VisualTestContext::from_window(window, cx);
    visual.simulate_keystrokes("/");
    visual.update(|window, cx| {
        assert!(
            app.read(cx).image_viewers[&id].0.read(cx).focus.is_focused(window),
            "chat shortcuts must not move focus behind the viewer"
        );
    });
    let count = visual.read(|cx| app.read(cx).chats.len());
    visual.simulate_keystrokes("secondary-n secondary-k secondary-p");
    visual.read(|cx| {
        assert_eq!(app.read(cx).chats.len(), count, "New Chat must not run behind the viewer");
        assert!(!app.read(cx).palette_open);
    });
    visual.simulate_keystrokes("escape");
    visual.update(|window, cx| {
        let app = app.read(cx);
        assert!(!app.image_viewers[&id].0.read(cx).visible);
        assert!(!app.chats[app.active].stopped, "Escape must not stop generation");
        assert!(previous.is_focused(window), "restore the original composer focus");
    });
    // The installed local-link handler also previews images in direct chats.
    visual.update(|window, cx| {
        let open = cx.global::<aether_ui::OpenFileLink>().0.clone();
        assert!(open(
            aether_ui::FileLink { path: path.clone(), line: None, col: None },
            window,
            cx
        ));
        assert!(app.read(cx).image_viewers[&id].0.read(cx).visible);
    });
    let pane = visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            let pane = app.workspace.model.pane_for(app.chats[app.active].id).unwrap();
            app.detach_session(pane, window, cx);
            pane
        })
    });
    visual.run_until_parked();
    let detached = visual.read(|cx| {
        let app = app.read(cx);
        let host = app.workspace.model.window_of(pane).unwrap();
        app.workspace.windows[&host]
    });
    let detached_id = visual
        .update_window(detached, |_, window, cx| {
            app.update(cx, |app, cx| {
                assert!(app.preview_image(&path, window, cx));
                assert!(app.image_viewer_overlay(window, cx).is_some());
                window.window_handle().window_id()
            })
        })
        .unwrap();
    assert_ne!(id, detached_id);
    visual.update(|window, cx| {
        app.update(cx, |app, cx| {
            let viewer = app.image_viewers[&id].0.clone();
            viewer.update(cx, |viewer, cx| viewer.close(window, cx));
            assert!(
                app.image_viewers[&detached_id].0.read(cx).visible,
                "closing the main viewer leaves the detached viewer open"
            );
        });
    });
    visual.run_until_parked();
    let count = visual.read(|cx| app.read(cx).chats.len());
    visual.simulate_keystrokes("secondary-n");
    visual.read(|cx| {
        assert_eq!(
            app.read(cx).chats.len(),
            count + 1,
            "a detached viewer must not block main-window shortcuts"
        );
    });
    std::fs::remove_dir_all(dir).unwrap();
}

#[gpui_kit::test]
fn image_viewer_newest_load_wins_and_closing_releases_pixels(cx: &mut gpui_kit::TestAppContext) {
    use super::ImageViewer;
    use gpui_kit::AppContext;
    cx.background_executor.allow_parking();
    cx.update(gpui_kit::init);
    let dir = test_directory();
    let path = dir.join("latest.png");
    image::RgbaImage::new(31, 17).save(&path).unwrap();
    let (window, viewer) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |_, cx| cx.new(ImageViewer::new)).unwrap()
    });
    cx.update_window(window, |_, window, cx| {
        viewer.update(cx, |viewer, cx| {
            viewer.open(dir.join("missing.png"), window, cx);
            viewer.open(path.clone(), window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let viewer = viewer.read(cx);
        assert_eq!(viewer.path, path);
        assert_eq!(viewer.loaded.as_ref().unwrap().dimensions, [31, 17]);
        assert!(viewer.error.is_none());
    });
    cx.update_window(window, |_, window, cx| {
        viewer.update(cx, |viewer, cx| {
            viewer.open(path.clone(), window, cx);
            viewer.close(window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let viewer = viewer.read(cx);
        assert!(!viewer.visible);
        assert!(viewer.loaded.is_none());
        assert!(viewer.error.is_none());
    });
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn supported_raster_formats_decode_and_gifs_use_the_first_frame() {
    let dir = test_directory();
    for ext in ["jpg", "webp", "tiff", "bmp"] {
        let path = dir.join(format!("sample.{ext}"));
        image::RgbImage::from_pixel(3, 2, image::Rgb([20, 40, 60])).save(&path).unwrap();
        assert_eq!(load(&path).unwrap().dimensions, [3, 2], "{ext}");
    }
    let path = dir.join("animated.gif");
    {
        let mut encoder =
            image::codecs::gif::GifEncoder::new(std::fs::File::create(&path).unwrap());
        for pixel in [[255, 0, 0, 255], [0, 0, 255, 255]] {
            encoder
                .encode_frame(image::Frame::new(image::RgbaImage::from_pixel(
                    2,
                    1,
                    image::Rgba(pixel),
                )))
                .unwrap();
        }
    }
    let loaded = load(&path).unwrap();
    assert_eq!(loaded.pixels.frame_count(), 1);
    assert_eq!(&loaded.pixels.as_bytes(0).unwrap()[..4], &[0, 0, 255, 255]);
    std::fs::remove_dir_all(dir).unwrap();
}
