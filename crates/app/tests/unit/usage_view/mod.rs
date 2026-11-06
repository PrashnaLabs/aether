use super::*;

#[test]
fn usage_provider_icon_and_color_cover_all_six() {
    let mut paths: Vec<_> = UsageProvider::ALL.into_iter().map(usage_provider_icon_path).collect();
    for p in &paths {
        assert!(p.starts_with("icons/provider-") && p.ends_with(".svg"));
    }
    paths.sort();
    paths.dedup();
    assert_eq!(paths.len(), UsageProvider::COUNT);
}

#[test]
fn provider_labels_have_readable_contrast_in_both_themes() {
    fn luminance(color: Hsla) -> f32 {
        let c = gpui_kit::Rgba::from(color);
        let linear = |v: f32| {
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
    }

    for (dark, backgrounds) in [(false, [0xffffff, 0xf4f4f5]), (true, [0x09090b, 0x18181b])] {
        for provider in UsageProvider::ALL {
            let fg = luminance(usage_provider_color(provider, dark));
            for bg in backgrounds {
                let bg = luminance(rgb(bg).into());
                let contrast = (fg.max(bg) + 0.05) / (fg.min(bg) + 0.05);
                assert!(contrast >= 4.5, "{provider:?}, dark={dark}: contrast {contrast:.2}");
            }
        }
    }
}
