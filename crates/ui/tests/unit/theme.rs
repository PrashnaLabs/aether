use std::rc::Rc;

use aether_core::Appearance;
use gpui_kit::component::{Theme, ThemeConfig};
use gpui_kit::{AppContext, Context, IntoElement, Render, VisualTestContext, div};

use super::*;

#[test]
fn legible_only_moves_colors_that_fail() {
    let white: Hsla = rgb(0xffffff).into();
    let green: Hsla = rgb(0x40a02b).into();
    assert!(contrast(legible(green, white), white) >= 4.5);
    let dark_green: Hsla = rgb(0x15803d).into();
    assert_eq!(legible(dark_green, white), dark_green);
}

#[test]
fn semantic_text_keeps_stock_light_and_fixes_schemes() {
    let mut theme = Theme::default();
    theme
        .apply_config(&Rc::new(ThemeConfig { name: "Default Light".into(), ..Default::default() }));
    let stock = semantic_text(&theme, theme.success, 0x15803D);
    assert_eq!(stock, Hsla::from(rgb(0x15803D)));
    theme.apply_config(&crate::schemes::scheme_config(Appearance::CatppuccinLatte).unwrap());
    assert!(contrast(semantic_text(&theme, theme.success, 0x15803D), theme.background) >= 4.5);
}

#[test]
fn scale_length_keeps_units() {
    assert_eq!(scale_length(px(10.).into(), 1.0), AbsoluteLength::Pixels(px(10.)));
    assert_eq!(scale_length(px(10.).into(), 0.0), AbsoluteLength::Pixels(px(0.)));
    assert_eq!(scale_length(rems(0.5).into(), 1.5), AbsoluteLength::Rems(rems(0.75)));
}

#[test]
fn surface_opacity_does_not_compound_and_skips_popovers() {
    let mut colors = Theme::default().colors;
    colors.popover.a = 1.0;
    apply_surface_opacity(&mut colors, 80);
    apply_surface_opacity(&mut colors, 70);
    assert!((colors.background.a - 0.7).abs() < 0.001);
    assert!((colors.sidebar.a - 0.7).abs() < 0.001);
    assert_eq!(colors.popover.a, 1.0);
}

#[test]
fn window_background_stays_opaque_at_full_opacity() {
    use WindowBackgroundAppearance as W;
    assert_eq!(window_background(100, true), W::Opaque);
    assert_eq!(window_background(80, false), W::Transparent);
    let blurred = if cfg!(target_os = "macos") { W::Blurred } else { W::Transparent };
    assert_eq!(window_background(80, true), blurred);
}

struct Blank;

impl Render for Blank {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[gpui::test]
fn alert_dialogs_stay_opaque_below_full_opacity(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
    });
    let (handle, _) = cx
        .update(|cx| gpui_kit::open_window(Default::default(), cx, |_, cx| cx.new(|_| Blank)))
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        Theme::update(cx, |theme| apply_surface_opacity(&mut theme.colors, 70));
        open_alert(window, cx, |alert, _, _| alert.title("Sure?"));
    })
    .unwrap();
    let mut visual = VisualTestContext::from_window(handle, cx);
    visual.update(|window, cx| {
        window.simulate_next_frame(cx);
    });
    let dialog = visual.debug_bounds("dialog-0").unwrap();
    visual.update(|window, cx| {
        let page = cx.theme().background;
        assert!((page.a - 0.7).abs() < 0.001);
        let scale = window.scale_factor();
        let same = |a: gpui_kit::ScaledPixels, b: Pixels| (a.0 - f32::from(b) * scale).abs() < 0.5;
        let quad = window
            .painted_quads()
            .into_iter()
            .find(|q| {
                same(q.bounds.size.width, dialog.size.width)
                    && same(q.bounds.size.height, dialog.size.height)
            })
            .expect("dialog surface quad");
        assert_eq!(quad.background.as_solid(), Some(page.alpha(1.)));
    });
}
