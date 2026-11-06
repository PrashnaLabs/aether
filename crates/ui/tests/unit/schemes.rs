use aether_core::{Appearance, THEME_FAMILIES, family_index};
use gpui_kit::WindowAppearance;
use gpui_kit::component::Theme;
use gpui_kit::{Hsla, rgb};

use super::*;
use crate::theme::contrast;

fn themed(a: Appearance) -> Theme {
    let mut theme = Theme::default();
    theme.apply_config(&scheme_config(a).unwrap());
    theme
}

#[test]
fn every_scheme_applies_and_stays_legible() {
    for a in Appearance::ALL.into_iter().filter(|a| scheme_palette(*a).is_some()) {
        let t = themed(a);
        assert_eq!(t.is_dark(), a.is_dark().unwrap(), "{a:?}");
        assert_eq!(t.theme_name().as_ref(), a.label());
        assert!(contrast(t.foreground, t.background) >= 4.5, "{a:?}");
        assert!(contrast(t.muted_foreground, t.background) >= 3.0, "{a:?}");
        assert!(contrast(t.primary_foreground, t.primary) >= 3.0, "{a:?}");
    }
}

#[test]
fn every_color_key_exists_in_the_kit_schema() {
    let keys = color_keys(&NORD);
    let parsed = scheme_config(Appearance::Nord).unwrap();
    let known = serde_json::to_value(&parsed.colors).unwrap();
    let dead: Vec<_> = keys.keys().filter(|k| known[k.as_str()].is_null()).collect();
    assert!(dead.is_empty(), "not in schema: {dead:?}");
}

#[test]
fn every_highlight_key_exists_in_the_kit_schema() {
    let keys = highlight_keys(&NORD);
    let parsed = scheme_config(Appearance::Nord).unwrap();
    let known = serde_json::to_value(parsed.highlight.as_ref().unwrap()).unwrap();
    let mut dead: Vec<_> =
        keys.as_object().unwrap().keys().filter(|k| known[k.as_str()].is_null()).collect();
    dead.extend(
        keys["syntax"]
            .as_object()
            .unwrap()
            .keys()
            .filter(|k| known["syntax"][k.as_str()].is_null()),
    );
    assert!(dead.is_empty(), "not in schema: {dead:?}");
}

#[test]
fn mapped_fields_take_effect_in_every_scheme() {
    let solid = |c: u32| Hsla::from(rgb(c));
    for a in Appearance::ALL.into_iter().filter(|a| scheme_palette(*a).is_some()) {
        let (t, c) = (themed(a), scheme_palette(a).unwrap());
        for (name, field, want) in [
            ("border", t.border, c.border),
            ("danger", t.danger, c.red),
            ("link", t.link, c.primary),
            ("link_hover", t.link_hover, c.primary),
            ("link_active", t.link_active, c.primary),
            ("drag_border", t.drag_border, c.primary),
            ("slider_bar", t.slider_bar, c.primary),
            ("progress_bar", t.progress_bar, c.primary),
        ] {
            assert_eq!(field, solid(want), "{a:?} {name}");
        }
        assert_eq!(t.selection.alpha(1.), solid(c.primary).alpha(1.), "{a:?} selection");
        assert!(t.selection.a < 1., "{a:?} selection");
    }
}

#[test]
fn stock_appearances_have_no_scheme() {
    for a in [Appearance::System, Appearance::Dark, Appearance::Light] {
        assert!(scheme_config(a).is_none());
    }
}

#[test]
fn forced_appearance_clears_for_system() {
    assert_eq!(forced_appearance(Appearance::System), None);
    assert_eq!(forced_appearance(Appearance::Nord), Some(WindowAppearance::Dark));
    assert_eq!(forced_appearance(Appearance::RosePineDawn), Some(WindowAppearance::Light));
}

#[gpui::test]
fn dark_to_light_scheme_resets_the_dark_slot(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        install_scheme(Appearance::Nord, cx);
        assert_eq!(Theme::global(cx).dark_theme.name.as_ref(), "Nord");
        install_scheme(Appearance::CatppuccinLatte, cx);
        assert_eq!(Theme::global(cx).dark_theme.name.as_ref(), "Default Dark");
        assert_eq!(Theme::global(cx).light_theme.name.as_ref(), "Catppuccin Latte");
    });
}

#[test]
fn thumbnails_split_dark_left_light_right_with_shown_on_its_side() {
    use Appearance::*;
    let sides = |a| thumbnail_sides(&THEME_FAMILIES[family_index(a)], a);
    assert_eq!(sides(System), (Dark, Some(Light)));
    assert_eq!(sides(Dark), (Dark, None));
    assert_eq!(sides(Light), (Light, None));
    assert_eq!(sides(Nord), (Nord, None));
    assert_eq!(sides(CatppuccinFrappe), (CatppuccinFrappe, Some(CatppuccinLatte)));
    assert_eq!(sides(CatppuccinMocha), (CatppuccinMocha, Some(CatppuccinLatte)));
    assert_eq!(sides(CatppuccinLatte), (CatppuccinFrappe, Some(CatppuccinLatte)));
    assert_eq!(sides(RosePineDawn), (RosePineMain, Some(RosePineDawn)));
    assert_eq!(sides(GruvboxLight), (GruvboxDark, Some(GruvboxLight)));
    assert_eq!(sides(SolarizedDark), (SolarizedDark, Some(SolarizedLight)));
}
