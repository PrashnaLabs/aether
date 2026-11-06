use super::*;
use alacritty_terminal::event::VoidListener;
use alacritty_terminal::term::Config;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::vte::ansi::Processor;

const FG: Rgb = rgb(0x0a0a0a);
const BG: Rgb = rgb(0xffffff);

fn light() -> Palette {
    Palette::new(false, FG, BG)
}

fn term(bytes: &[u8], cols: usize, rows: usize) -> Term<VoidListener> {
    let mut term = Term::new(Config::default(), &TermSize::new(cols, rows), VoidListener);
    Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::new().advance(&mut term, bytes);
    term
}

#[test]
fn ansi_colors_stay_readable_on_both_themes() {
    for (palette, bg) in [(LIGHT, BG), (DARK, rgb(0x0a0a0a))] {
        for i in (1..=14).filter(|&i| i != 7 || bg == BG) {
            assert!(contrast(palette[i], bg) >= 4.5, "color {i} {:?}", palette[i]);
        }
    }
}

#[test]
fn indexed_colors_cover_cube_ramp_and_named_slots() {
    let p = light();
    assert_eq!(p.index(1), LIGHT[1]);
    assert_eq!(p.index(16), rgb(0x000000));
    assert_eq!(p.index(196), rgb(0xff0000));
    assert_eq!(p.index(110), rgb(0x87afd7));
    assert_eq!(p.index(231), rgb(0xffffff));
    assert_eq!(p.index(232), rgb(0x080808));
    assert_eq!(p.index(236), rgb(0x303030));
    assert_eq!(p.index(255), rgb(0xeeeeee));
    assert_eq!(p.index(NamedColor::Foreground as usize), FG);
    assert_eq!(p.index(NamedColor::Background as usize), BG);
    assert_eq!(p.index(NamedColor::DimRed as usize), mix(LIGHT[1], BG, DIM));
    assert_eq!(p.index(NamedColor::DimWhite as usize), mix(LIGHT[7], BG, DIM));
    assert_eq!(p.index(NamedColor::DimForeground as usize), mix(FG, BG, DIM));
}

#[test]
fn palette_from_theme_and_indices_256_257() {
    let theme_palette = ThemePalette::mocha();
    let palette = Palette::from_theme(theme_palette);
    assert_eq!(palette.fg, theme_palette.fg);
    assert_eq!(palette.bg, theme_palette.bg);
    assert_eq!(palette.cursor, theme_palette.cursor);
    assert_eq!(palette.selection, theme_palette.selection);
    assert_eq!(palette.index(256), theme_palette.fg);
    assert_eq!(palette.index(257), theme_palette.bg);
    assert_eq!(palette.index(258), theme_palette.cursor);
}

#[test]
fn programs_can_override_palette_entries() {
    let p = light();
    let mut overrides = Colors::default();
    overrides[NamedColor::Red] = Some(rgb(0x123456));
    assert_eq!(p.resolve(Color::Named(NamedColor::Red), &overrides), rgb(0x123456));
    assert_eq!(p.resolve(Color::Indexed(1), &overrides), rgb(0x123456));
    assert_eq!(p.resolve(Color::Indexed(2), &overrides), LIGHT[2]);
    assert_eq!(p.resolve(Color::Spec(rgb(0xabcdef)), &overrides), rgb(0xabcdef));
}

#[test]
fn inverse_faint_and_hidden_cells() {
    let (p, o) = (light(), Colors::default());
    let fg = Color::Named(NamedColor::Foreground);
    let bg = Color::Named(NamedColor::Background);
    assert_eq!(cell_colors(fg, bg, Flags::empty(), &p, &o), (FG, None));
    assert_eq!(cell_colors(fg, bg, Flags::INVERSE, &p, &o), (BG, Some(FG)));
    let red = Color::Named(NamedColor::Red);
    assert_eq!(
        cell_colors(red, Color::Indexed(4), Flags::INVERSE, &p, &o),
        (LIGHT[4], Some(LIGHT[1]))
    );
    assert_eq!(cell_colors(fg, bg, Flags::DIM, &p, &o).0, mix(FG, BG, DIM));
    assert_eq!(cell_colors(fg, bg, Flags::DIM_BOLD, &p, &o).0, mix(FG, BG, DIM));
    // faint text on a colored cell fades toward that cell, not the terminal background
    assert_eq!(
        cell_colors(fg, Color::Indexed(4), Flags::DIM, &p, &o),
        (mix(FG, LIGHT[4], DIM), Some(LIGHT[4]))
    );
    assert_eq!(
        cell_colors(fg, Color::Indexed(4), Flags::HIDDEN, &p, &o),
        (LIGHT[4], Some(LIGHT[4]))
    );
    // low-contrast pairs stay as the program sent them, like xterm and Ghostty
    let (yellow, cyan) = (Color::Indexed(3), Color::Indexed(6));
    assert_eq!(cell_colors(yellow, cyan, Flags::empty(), &p, &o), (LIGHT[3], Some(LIGHT[6])));
}

#[test]
fn sgr_sequences_render_as_xterm_does() {
    let p = Palette::from_theme(ThemePalette::mocha());
    // nord-tmux window tab (white on bright black), faint, 256-color gray, truecolor
    let t = term(b"\x1b[37;100mA\x1b[0;2mB\x1b[0;38;5;236mC\x1b[0;38;2;1;2;3;48;2;4;5;6mD", 20, 1);
    let row = &snapshot(&t, &p, false).rows[0];
    let fgs: Vec<Rgb> = row.runs.iter().map(|r| r.style.fg).collect();
    assert_eq!(fgs, [p.index(7), mix(p.fg, p.bg, DIM), rgb(0x303030), rgb(0x010203)]);
    assert_eq!(row.bgs, [(0, 1, p.index(8)), (3, 4, rgb(0x040506))]);
}

#[test]
fn text_on_a_background_reaches_readable_contrast() {
    let (white, black) = (rgb(0xffffff), rgb(0x000000));
    for (palette, bg) in [(LIGHT, BG), (DARK, rgb(0x0a0a0a))] {
        let p = Palette::new(palette == DARK, FG, bg);
        for f in 0..16 {
            for b in 0..16 {
                let (fg, back) = (p.index(f), p.index(b));
                let floor = MIN_CONTRAST.min(contrast(white, back).max(contrast(black, back)));
                let got = contrast(readable(fg, back), back);
                assert!(got >= floor - 0.01, "fg {f} on bg {b}: {got}");
            }
        }
    }
    // already readable text keeps its color
    assert_eq!(readable(LIGHT[1], BG), LIGHT[1]);
    // a red FAIL badge keeps white text
    assert_eq!(readable(rgb(0xffffff), LIGHT[1]), rgb(0xffffff));
}

#[test]
fn rows_merge_equal_styles_and_backgrounds() {
    let plain = Style {
        fg: FG,
        bold: false,
        italic: false,
        underline: false,
        undercurl: false,
        strike: false,
    };
    let bold = Style { bold: true, ..plain };
    let mut row = Row::default();
    row.push(0, 'a', &[], plain, None);
    row.push(1, 'b', &[], plain, Some(BG));
    row.push(2, 'e', &['\u{301}'], bold, Some(BG));
    row.push(3, ' ', &[], bold, Some(FG));
    row.push(4, ' ', &[], bold, None);
    assert_eq!(row.runs, [Run { len: 2, style: plain }, Run { len: 5, style: bold }]);
    assert_eq!(row.bgs, [(1, 3, BG), (3, 4, FG)]);
    row.trim_end();
    assert_eq!(row.text, "abe\u{301}");
    assert_eq!(row.runs[1].len, 3);

    let mut underlined = Row::default();
    underlined.push(0, ' ', &[], Style { underline: true, ..plain }, None);
    underlined.trim_end();
    assert_eq!(underlined.text, " ");
}

#[test]
fn snapshot_reads_text_colors_and_wide_chars() {
    let p = light();
    let t = term("\x1b[31mred\x1b[0m ok\r\na中b\x1b[7mx".as_bytes(), 20, 3);
    let frame = snapshot(&t, &p, false);
    assert_eq!(frame.rows[0].text, "red ok");
    assert_eq!(
        frame.rows[0].runs[0],
        Run {
            len: 3,
            style: Style {
                fg: LIGHT[1],
                bold: false,
                italic: false,
                underline: false,
                undercurl: false,
                strike: false
            }
        }
    );
    assert_eq!(frame.rows[0].runs[1].len, 3);
    // the spacer after 中 keeps later glyphs on their own columns
    assert_eq!(frame.rows[1].text, "a中 bx");
    assert_eq!(frame.rows[1].bgs, [(4, 5, FG)]);
    assert!(!frame.blinking);
    assert_eq!(frame.rows[2].text, "");
    assert_eq!(
        frame.cursor,
        Some(FrameCursor { row: 1, col: 5, width: 1, shape: CursorShape::Block })
    );
}

#[test]
fn block_cursor_text_takes_the_background() {
    let p = light();
    let t = term(b"ab\x1b[D", 20, 2);
    assert_eq!(snapshot(&t, &p, true).rows[0].runs.last().unwrap().style.fg, BG);
    assert_eq!(snapshot(&t, &p, false).rows[0].runs.last().unwrap().style.fg, FG);
    let hidden = term(b"ab\x1b[?25l", 20, 2);
    assert_eq!(snapshot(&hidden, &p, true).cursor, None);
}

#[test]
fn grid_fits_the_drawer_with_a_floor() {
    assert_eq!(grid_size(800., 216., 7.5, 18.), (106, 12));
    assert_eq!(grid_size(100., 50., 7.5, 18.), (MIN_COLS, MIN_ROWS));
    assert_eq!(grid_size(-5., 0., 0., 18.), (MIN_COLS, MIN_ROWS));
}

#[test]
fn short_drawers_keep_the_cursor_row_in_view() {
    assert_eq!(first_visible_row(8, 12, Some(7)), 0);
    assert_eq!(first_visible_row(8, 6, Some(0)), 0);
    assert_eq!(first_visible_row(8, 6, Some(6)), 1);
    assert_eq!(first_visible_row(8, 6, Some(7)), 2);
    assert_eq!(first_visible_row(8, 6, None), 2);
}

#[test]
fn search_lines_finds_case_insensitive_matches_and_ignores_empty_queries() {
    let lines = vec![
        (-2, "Compiling aether v0.1.0"),
        (-1, "Finished `dev` profile in 1.42s"),
        (0, "running 3 tests ... finished ok"),
    ];
    assert!(search_lines(lines.clone(), "").is_empty());
    assert!(search_lines(lines.clone(), "   ").is_empty());
    assert!(search_lines(lines.clone(), "nonexistent").is_empty());

    let hits = search_lines(lines, "FINISHED");
    assert_eq!(
        hits,
        vec![
            SearchMatch { line: -1, start_col: 0, end_col: 8 },
            SearchMatch { line: 0, start_col: 20, end_col: 28 },
        ]
    );
}

#[test]
fn display_offset_for_line_scrolls_history_line_into_view() {
    // 24 screen lines, 100 lines of scrollback history
    assert_eq!(display_offset_for_line(5, 24, 100, 0), 0);
    // Line -30 is 30 lines above viewport top (0); centers or brings into viewport clamped to history_size
    let offset = display_offset_for_line(-30, 24, 100, 0);
    assert!((30..=100).contains(&offset));
}

#[test]
fn search_term_and_snapshot_highlight_matches() {
    let p = light();
    let t = term(b"First hit here\r\nsecond HIT there", 20, 2);
    let hits = search_term(&t, "hit");
    assert_eq!(
        hits,
        vec![
            SearchMatch { line: 0, start_col: 6, end_col: 9 },
            SearchMatch { line: 1, start_col: 7, end_col: 10 },
        ]
    );
    let frame = snapshot_search_matches(&t, &p, false, &hits, Some(1));
    assert_eq!(frame.rows[0].bgs, vec![(6, 9, SEARCH_MATCH_BG)]);
    assert_eq!(frame.rows[1].bgs, vec![(7, 10, ACTIVE_SEARCH_MATCH_BG)]);
}
