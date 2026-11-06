use super::*;

fn covers(rects: &[([f32; 4], f32)], x: f32, y: f32) -> bool {
    rects.iter().any(|([x0, y0, x1, y1], _)| (*x0..*x1).contains(&x) && (*y0..*y1).contains(&y))
}

#[test]
fn table_has_one_entry_per_character() {
    assert_eq!(LINES.len(), 128 * 4);
}

#[test]
fn lines_reach_the_cell_edges_they_point_to() {
    let (w, h) = (8., 18.);
    let horizontal = shape('─', w, h).unwrap();
    assert!(
        covers(&horizontal, 0., 9.) && covers(&horizontal, 7.9, 9.) && !covers(&horizontal, 4., 0.)
    );
    let vertical = shape('│', w, h).unwrap();
    assert!(covers(&vertical, 4., 0.) && covers(&vertical, 4., 17.9) && !covers(&vertical, 0., 9.));
    let corner = shape('┌', w, h).unwrap();
    assert!(
        covers(&corner, 7.9, 9.)
            && covers(&corner, 4., 17.9)
            && !covers(&corner, 0., 9.)
            && !covers(&corner, 4., 0.)
    );
    // rounded corners draw square so they meet straight neighbours
    assert_eq!(shape('╭', w, h), shape('┌', w, h));
    assert_eq!(shape('╋', w, h).unwrap().len(), 4);
    assert_eq!(shape('╬', w, h).unwrap().len(), 8);
    assert!(shape('━', w, h).unwrap()[0].0[3] - shape('━', w, h).unwrap()[0].0[1] > 1.);
}

#[test]
fn diagonals_and_other_text_stay_glyphs() {
    assert!(!is_box('╱') && !is_box('╳') && !is_box('a'));
    assert!(is_box('▒') && is_box('╿'));
}

#[test]
fn blocks_fill_their_eighths_and_shades_fade() {
    let (w, h) = (8., 16.);
    assert_eq!(shape('▄', w, h).unwrap(), [([0., 8., 8., 16.], 1.)]);
    assert_eq!(shape('▁', w, h).unwrap(), [([0., 14., 8., 16.], 1.)]);
    assert_eq!(shape('█', w, h).unwrap(), [([0., 0., 8., 16.], 1.)]);
    assert_eq!(shape('▏', w, h).unwrap(), [([0., 0., 1., 16.], 1.)]);
    assert_eq!(shape('▐', w, h).unwrap(), [([4., 0., 8., 16.], 1.)]);
    assert_eq!(shape('░', w, h).unwrap()[0].1, 0.25);
    assert_eq!(shape('▓', w, h).unwrap()[0].1, 0.75);
    let checker = shape('▚', w, h).unwrap();
    assert!(covers(&checker, 1., 1.) && covers(&checker, 7., 15.) && !covers(&checker, 7., 1.));
    assert_eq!(shape('▟', w, h).unwrap().len(), 3);
}
