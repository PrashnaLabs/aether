//! Box-drawing (U+2500-257F) and block (U+2580-259F) characters drawn as
//! rectangles, so lines meet across cells whatever the font's glyph metrics.

/// Arms of U+2500..=U+257F as `up right down left`: 0 none, 1 light, 2 heavy,
/// 3 double. Dashed lines draw solid; `-` (diagonals) are left to the font.
const LINES: &str = "\
0101020210102020010102021010202001010202101020200110021001200220\
0011001200210022110012002100220010011002200120021110121021101120\
2120221012202220101110122011102120212012102220220111011202110212\
0121012202210222110111021201120221012102220122021111111212111212\
2111112121212112221111221221221212222122222122220101020210102020\
0303303003100130033000130031003313003100330010033001300313103130\
3330101330313033031301310333130331013303131331313333011000111001\
1100------------000110000100001000022000020000200201102001022010";

/// Rectangles `[x0, y0, x1, y1]` in px from the cell's top-left, with an
/// opacity, for a `w`×`h` cell; `None` when the font should draw `c`.
pub(crate) fn shape(c: char, w: f32, h: f32) -> Option<Vec<([f32; 4], f32)>> {
    let code = c as u32;
    match code {
        0x2500..=0x257F => {
            lines(LINES.as_bytes().get((code - 0x2500) as usize * 4..)?.get(..4)?, w, h)
        }
        0x2580..=0x259F => Some(block(code, w, h)),
        _ => None,
    }
}

pub(crate) fn is_box(c: char) -> bool {
    shape(c, 1., 1.).is_some()
}

fn lines(arms: &[u8], w: f32, h: f32) -> Option<Vec<([f32; 4], f32)>> {
    if arms[0] == b'-' {
        return None;
    }
    let t = (h / 18.).round().max(1.);
    let (cx, cy) = ((w / 2.).floor(), (h / 2.).floor());
    let weight = |side: usize| arms[side] - b'0';
    // half the width a line of this weight takes across its axis
    let half = |weight: u8| [0., t / 2., t, 1.5 * t][weight as usize];
    // arms run into the center just far enough to cover the arms crossing them
    let (reach_h, reach_v) =
        (half(weight(0)).max(half(weight(2))), half(weight(1)).max(half(weight(3))));
    let mut out = Vec::new();
    for side in 0..4 {
        let weight = weight(side);
        if weight == 0 {
            continue;
        }
        let thick = if weight == 2 { 2. * t } else { t };
        let offsets: &[f32] = if weight == 3 { &[-t, t] } else { &[0.] };
        for &o in offsets {
            let (near, far) = (o - thick / 2., o + thick / 2.);
            let rect = match side {
                0 => [cx + near, 0., cx + far, cy + reach_v],
                1 => [cx - reach_h, cy + near, w, cy + far],
                2 => [cx + near, cy - reach_v, cx + far, h],
                _ => [0., cy + near, cx + reach_h, cy + far],
            };
            out.push((rect, 1.));
        }
    }
    Some(out)
}

fn block(code: u32, w: f32, h: f32) -> Vec<([f32; 4], f32)> {
    let rect = |x0: u32, y0: u32, x1: u32, y1: u32| {
        [w * x0 as f32 / 8., h * y0 as f32 / 8., w * x1 as f32 / 8., h * y1 as f32 / 8.]
    };
    const UL: u8 = 1;
    const UR: u8 = 2;
    const LL: u8 = 4;
    const LR: u8 = 8;
    let quadrants = |mask: u8| {
        [
            (UL, rect(0, 0, 4, 4)),
            (UR, rect(4, 0, 8, 4)),
            (LL, rect(0, 4, 4, 8)),
            (LR, rect(4, 4, 8, 8)),
        ]
        .into_iter()
        .filter(|(q, _)| mask & q != 0)
        .map(|(_, r)| (r, 1.))
        .collect()
    };
    match code {
        0x2580 => vec![(rect(0, 0, 8, 4), 1.)],
        0x2581..=0x2588 => vec![(rect(0, 8 - (code - 0x2580), 8, 8), 1.)],
        0x2589..=0x258F => vec![(rect(0, 0, 0x2590 - code, 8), 1.)],
        0x2590 => vec![(rect(4, 0, 8, 8), 1.)],
        0x2591..=0x2593 => vec![(rect(0, 0, 8, 8), (code - 0x2590) as f32 / 4.)],
        0x2594 => vec![(rect(0, 0, 8, 1), 1.)],
        0x2595 => vec![(rect(7, 0, 8, 8), 1.)],
        _ => quadrants(
            [
                LL,
                LR,
                UL,
                UL | LL | LR,
                UL | LR,
                UL | UR | LL,
                UL | UR | LR,
                UR,
                UR | LL,
                UR | LL | LR,
            ][(code - 0x2596) as usize],
        ),
    }
}

#[cfg(test)]
#[path = "../tests/unit/boxes.rs"]
mod tests;
