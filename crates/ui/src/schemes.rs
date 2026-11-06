use crate::theme::Corners;
use std::rc::Rc;

use aether_core::{Appearance, ThemeFamily};
use gpui_kit::component::{Theme, ThemeConfig, ThemeRegistry, h_flex, v_flex};
use gpui_kit::{App, Div, Hsla, WindowAppearance, div, prelude::*, px, relative, rgb};
use serde_json::{Map, Value, json};

pub struct SchemePalette {
    pub bg: u32,
    pub panel: u32,
    pub raised: u32,
    pub border: u32,
    pub fg: u32,
    pub muted: u32,
    pub primary: u32,
    pub on_primary: u32,
    pub red: u32,
    pub green: u32,
    pub yellow: u32,
    pub orange: u32,
    pub blue: u32,
    pub magenta: u32,
    pub cyan: u32,
}

const fn p(c: [u32; 15]) -> SchemePalette {
    SchemePalette {
        bg: c[0],
        panel: c[1],
        raised: c[2],
        border: c[3],
        fg: c[4],
        muted: c[5],
        primary: c[6],
        on_primary: c[7],
        red: c[8],
        green: c[9],
        yellow: c[10],
        orange: c[11],
        blue: c[12],
        magenta: c[13],
        cyan: c[14],
    }
}

static LATTE: SchemePalette = p([
    0xeff1f5, 0xe6e9ef, 0xccd0da, 0xbcc0cc, 0x4c4f69, 0x6c6f85, 0x8839ef, 0xeff1f5, 0xd20f39,
    0x40a02b, 0xdf8e1d, 0xfe640b, 0x1e66f5, 0xea76cb, 0x179299,
]);
static FRAPPE: SchemePalette = p([
    0x303446, 0x292c3c, 0x414559, 0x51576d, 0xc6d0f5, 0xa5adce, 0xca9ee6, 0x303446, 0xe78284,
    0xa6d189, 0xe5c890, 0xef9f76, 0x8caaee, 0xf4b8e4, 0x81c8be,
]);
static MACCHIATO: SchemePalette = p([
    0x24273a, 0x1e2030, 0x363a4f, 0x494d64, 0xcad3f5, 0xa5adcb, 0xc6a0f6, 0x24273a, 0xed8796,
    0xa6da95, 0xeed49f, 0xf5a97f, 0x8aadf4, 0xf5bde6, 0x8bd5ca,
]);
static MOCHA: SchemePalette = p([
    0x1e1e2e, 0x181825, 0x313244, 0x45475a, 0xcdd6f4, 0xa6adc8, 0xcba6f7, 0x1e1e2e, 0xf38ba8,
    0xa6e3a1, 0xf9e2af, 0xfab387, 0x89b4fa, 0xf5c2e7, 0x94e2d5,
]);
static ROSE_MAIN: SchemePalette = p([
    0x191724, 0x1f1d2e, 0x26233a, 0x403d52, 0xe0def4, 0x908caa, 0xc4a7e7, 0x191724, 0xeb6f92,
    0x31748f, 0xf6c177, 0xebbcba, 0x9ccfd8, 0xc4a7e7, 0xebbcba,
]);
static ROSE_MOON: SchemePalette = p([
    0x232136, 0x2a273f, 0x393552, 0x44415a, 0xe0def4, 0x908caa, 0xc4a7e7, 0x232136, 0xeb6f92,
    0x3e8fb0, 0xf6c177, 0xea9a97, 0x9ccfd8, 0xc4a7e7, 0xea9a97,
]);
static ROSE_DAWN: SchemePalette = p([
    0xfaf4ed, 0xfffaf3, 0xf2e9e1, 0xdfdad9, 0x575279, 0x797593, 0x286983, 0xfaf4ed, 0xb4637a,
    0x286983, 0xea9d34, 0xd7827e, 0x56949f, 0x907aa9, 0xd7827e,
]);
static GRUVBOX_DARK: SchemePalette = p([
    0x282828, 0x1d2021, 0x3c3836, 0x504945, 0xebdbb2, 0xa89984, 0xfe8019, 0x282828, 0xfb4934,
    0xb8bb26, 0xfabd2f, 0xfe8019, 0x83a598, 0xd3869b, 0x8ec07c,
]);
static GRUVBOX_LIGHT: SchemePalette = p([
    0xfbf1c7, 0xf2e5bc, 0xebdbb2, 0xd5c4a1, 0x3c3836, 0x7c6f64, 0xaf3a03, 0xfbf1c7, 0x9d0006,
    0x79740e, 0xb57614, 0xaf3a03, 0x076678, 0x8f3f71, 0x427b58,
]);
static SOLARIZED_DARK: SchemePalette = p([
    0x002b36, 0x073642, 0x073642, 0x586e75, 0x93a1a1, 0x839496, 0x268bd2, 0x002b36, 0xdc322f,
    0x859900, 0xb58900, 0xcb4b16, 0x268bd2, 0xd33682, 0x2aa198,
]);
static SOLARIZED_LIGHT: SchemePalette = p([
    0xfdf6e3, 0xeee8d5, 0xeee8d5, 0x93a1a1, 0x586e75, 0x657b83, 0x268bd2, 0xfdf6e3, 0xdc322f,
    0x859900, 0xb58900, 0xcb4b16, 0x268bd2, 0xd33682, 0x2aa198,
]);
static NORD: SchemePalette = p([
    0x2e3440, 0x3b4252, 0x434c5e, 0x4c566a, 0xd8dee9, 0x9ca2ae, 0x88c0d0, 0x2e3440, 0xbf616a,
    0xa3be8c, 0xebcb8b, 0xd08770, 0x81a1c1, 0xb48ead, 0x8fbcbb,
]);
static DRACULA: SchemePalette = p([
    0x282a36, 0x21222c, 0x44475a, 0x44475a, 0xf8f8f2, 0xa5a6a7, 0xbd93f9, 0x282a36, 0xff5555,
    0x50fa7b, 0xf1fa8c, 0xffb86c, 0x8be9fd, 0xff79c6, 0x8be9fd,
]);
static STOCK_DARK: SchemePalette = p([
    0x0a0a0a, 0x171717, 0x262626, 0x262626, 0xfafafa, 0xa3a3a3, 0xfafafa, 0x171717, 0xf87171,
    0x4ade80, 0xfacc15, 0xfb923c, 0x60a5fa, 0xc084fc, 0x22d3ee,
]);
static STOCK_LIGHT: SchemePalette = p([
    0xffffff, 0xfafafa, 0xf5f5f5, 0xe5e5e5, 0x0a0a0a, 0x737373, 0x171717, 0xfafafa, 0xdc2626,
    0x15803d, 0xa16207, 0xea580c, 0x2563eb, 0x9333ea, 0x0e7490,
]);

pub fn scheme_palette(a: Appearance) -> Option<&'static SchemePalette> {
    use Appearance::*;
    Some(match a {
        System | Dark | Light => return None,
        CatppuccinLatte => &LATTE,
        CatppuccinFrappe => &FRAPPE,
        CatppuccinMacchiato => &MACCHIATO,
        CatppuccinMocha => &MOCHA,
        RosePineMain => &ROSE_MAIN,
        RosePineMoon => &ROSE_MOON,
        RosePineDawn => &ROSE_DAWN,
        GruvboxDark => &GRUVBOX_DARK,
        GruvboxLight => &GRUVBOX_LIGHT,
        SolarizedDark => &SOLARIZED_DARK,
        SolarizedLight => &SOLARIZED_LIGHT,
        Nord => &NORD,
        Dracula => &DRACULA,
    })
}

pub fn preview_palette(a: Appearance) -> &'static SchemePalette {
    scheme_palette(a).unwrap_or(match a {
        Appearance::Light => &STOCK_LIGHT,
        _ => &STOCK_DARK,
    })
}

fn put(m: &mut Map<String, Value>, keys: &[&str], c: u32, alpha: &str) {
    for k in keys {
        m.insert((*k).into(), json!(format!("#{c:06x}{alpha}")));
    }
}

fn color_keys(c: &SchemePalette) -> Map<String, Value> {
    let mut m = Map::new();
    let mut set = |keys: &[&str], color: u32| put(&mut m, keys, color, "");
    set(
        &[
            "background",
            "list.background",
            "table.background",
            "accordion.background",
            "group_box.background",
            "tab.active.background",
            "slider.thumb.background",
        ],
        c.bg,
    );
    set(
        &[
            "popover.background",
            "sidebar.background",
            "title_bar.background",
            "status_bar.background",
            "tab_bar.background",
            "tab_bar.segmented.background",
        ],
        c.panel,
    );
    set(
        &[
            "muted.background",
            "secondary.background",
            "secondary.hover.background",
            "secondary.active.background",
            "accent.background",
            "sidebar.accent.background",
            "skeleton.background",
            "switch.background",
        ],
        c.raised,
    );
    set(
        &[
            "border",
            "input.border",
            "sidebar.border",
            "title_bar.border",
            "status_bar.border",
            "window.border",
            "table.row.border",
        ],
        c.border,
    );
    set(
        &[
            "foreground",
            "caret",
            "popover.foreground",
            "group_box.foreground",
            "secondary.foreground",
            "accent.foreground",
            "sidebar.foreground",
            "sidebar.accent.foreground",
            "tab.active.foreground",
            "tab.foreground",
        ],
        c.fg,
    );
    set(
        &["muted.foreground", "table.head.foreground", "scrollbar.thumb.hover.background"],
        c.muted,
    );
    set(
        &[
            "primary.background",
            "primary.hover.background",
            "primary.active.background",
            "sidebar.primary.background",
            "ring",
            "slider.background",
            "progress.bar.background",
            "link",
            "link.hover",
            "link.active",
            "list.active.border",
            "drag.border",
        ],
        c.primary,
    );
    set(&["primary.foreground", "sidebar.primary.foreground"], c.on_primary);
    for (name, status, color) in [
        ("red", "danger", c.red),
        ("green", "success", c.green),
        ("yellow", "warning", c.yellow),
        ("blue", "info", c.blue),
    ] {
        set(&[&format!("{status}.background")], color);
        set(&[&format!("{status}.foreground")], c.on_primary);
        set(&[&format!("base.{name}"), &format!("base.{name}.light")], color);
    }
    for (name, color) in [("magenta", c.magenta), ("cyan", c.cyan)] {
        set(&[&format!("base.{name}"), &format!("base.{name}.light")], color);
    }
    put(&mut m, &["scrollbar.thumb.background"], c.muted, "99");
    put(&mut m, &["selection.background"], c.primary, "4d");
    put(&mut m, &["list.active.background"], c.primary, "33");
    put(&mut m, &["drop_target.background"], c.primary, "19");
    m
}

fn highlight_keys(c: &SchemePalette) -> Value {
    let hex = |c: u32| format!("#{c:06x}");
    let syn = |c: u32| json!({ "color": hex(c) });
    json!({
        "editor.background": hex(c.bg),
        "editor.foreground": hex(c.fg),
        "editor.active_line.background": hex(c.panel),
        "editor.line_number": hex(c.muted),
        "editor.active_line_number": hex(c.fg),
        "syntax": {
            "keyword": syn(c.magenta),
            "string": syn(c.green),
            "function": syn(c.blue),
            "type": syn(c.yellow),
            "constant": syn(c.orange),
            "number": syn(c.orange),
            "boolean": syn(c.orange),
            "comment": syn(c.muted),
            "attribute": syn(c.cyan),
            "tag": syn(c.blue),
            "property": syn(c.fg),
            "title": syn(c.orange),
        },
    })
}

pub fn scheme_config(a: Appearance) -> Option<Rc<ThemeConfig>> {
    let c = scheme_palette(a)?;
    let config = json!({
        "name": a.label(),
        "mode": if a.is_dark()? { "dark" } else { "light" },
        "colors": color_keys(c),
        "highlight": highlight_keys(c),
    });
    serde_json::from_value(config).ok().map(Rc::new)
}

#[cfg(test)]
#[path = "../tests/unit/schemes.rs"]
mod tests;

pub fn forced_appearance(a: Appearance) -> Option<WindowAppearance> {
    a.is_dark().map(|dark| if dark { WindowAppearance::Dark } else { WindowAppearance::Light })
}

pub fn install_scheme(a: Appearance, cx: &mut App) {
    let registry = ThemeRegistry::global(cx);
    let (light, dark) =
        (registry.default_light_theme().clone(), registry.default_dark_theme().clone());
    let scheme = scheme_config(a);
    Theme::update(cx, |theme| {
        theme.light_theme = light;
        theme.dark_theme = dark;
        if let Some(config) = scheme {
            if config.mode.is_dark() {
                theme.dark_theme = config;
            } else {
                theme.light_theme = config;
            }
        }
    });
}

pub fn thumbnail_sides(
    family: &ThemeFamily,
    shown: Appearance,
) -> (Appearance, Option<Appearance>) {
    if shown == Appearance::System {
        return (Appearance::Dark, Some(Appearance::Light));
    }
    let other = std::iter::once(&family.default)
        .chain(family.variants)
        .find(|a| a.is_dark() != shown.is_dark())
        .copied();
    match other {
        Some(other) if shown.is_dark() == Some(true) => (shown, Some(other)),
        Some(other) => (other, Some(shown)),
        None => (shown, None),
    }
}

fn miniature(a: Appearance) -> Div {
    let c = preview_palette(a);
    let color = |c: u32| Hsla::from(rgb(c));
    let bar = |w: f32| div().w(px(w)).h(px(5.)).rounded_full().bg(color(c.muted).opacity(0.6));
    h_flex()
        .size_full()
        .child(
            v_flex()
                .flex_none()
                .w(px(39.))
                .h_full()
                .pt(px(15.))
                .pl(px(9.))
                .gap(px(6.))
                .corner_l(px(11.))
                .bg(color(c.panel))
                .child(bar(22.))
                .child(bar(16.))
                .child(bar(22.)),
        )
        .child(
            v_flex()
                .flex_1()
                .h_full()
                .pt(px(15.))
                .px(px(7.))
                .corner_r(px(11.))
                .bg(color(c.bg))
                .child(bar(34.))
                .child(
                    h_flex().mt(px(7.)).gap(px(5.)).children(
                        [c.primary, c.raised, c.raised]
                            .map(|tile| div().flex_1().h(px(28.)).corner(px(4.)).bg(color(tile))),
                    ),
                )
                .child(
                    h_flex()
                        .mt(px(11.))
                        .h(px(14.))
                        .pl(px(5.))
                        .rounded_full()
                        .bg(color(c.raised))
                        .child(div().size(px(5.)).rounded_full().bg(color(c.primary))),
                ),
        )
}

pub fn scheme_thumbnail(left: Appearance, right: Option<Appearance>) -> impl IntoElement {
    div().relative().w_full().h(px(95.)).child(miniature(left)).when_some(right, |thumb, right| {
        thumb.child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .right_0()
                .w(relative(0.5))
                .overflow_hidden()
                .child(miniature(right).absolute().top_0().right_0().w(relative(2.))),
        )
    })
}
