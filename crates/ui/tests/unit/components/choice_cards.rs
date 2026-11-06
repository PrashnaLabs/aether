use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::{Context, Render, VisualTestContext};

use super::*;

struct Row;

impl Render for Row {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(400.)).child(CardCarousel::new("row", 0).children((0..9).map(|_| div())))
    }
}

#[gpui::test]
fn arrows_catch_up_with_layout_on_the_next_frame_by_themselves(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|_, _| Row);
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(*window, cx);
    assert!(visual.debug_bounds("row-next").is_none());
    visual.update(|window, cx| {
        window.simulate_next_frame(cx);
    });
    assert!(visual.debug_bounds("row-next").is_some());
    assert!(visual.debug_bounds("row-prev").is_none());
}

struct Cards;

impl Render for Cards {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(400.)).child(CardRow::new([
            ChoiceCard::new("short", "A"),
            ChoiceCard::new("long", "A label much longer than its neighbour's"),
        ]))
    }
}

#[gpui::test]
fn card_row_gives_cards_equal_width_whatever_their_labels(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_kit::init);
    let window = cx.add_window(|_, _| Cards);
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(*window, cx);
    let short = visual.debug_bounds("short").unwrap();
    let long = visual.debug_bounds("long").unwrap();
    assert_eq!(short.size.width, long.size.width);
    assert!(long.right() <= px(400.));
}

struct Clicks {
    card: Rc<Cell<u32>>,
    menu: Rc<Cell<u32>>,
}

impl Render for Clicks {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let on_card = |clicks: &Rc<Cell<u32>>| {
            let clicks = clicks.clone();
            move |_: &ClickEvent, _: &mut Window, _: &mut App| clicks.set(clicks.get() + 1)
        };
        let menu = self.menu.clone();
        div().w(px(400.)).child(CardRow::new([
            ChoiceCard::new("plain", "Nord").on_click(on_card(&self.card)),
            ChoiceCard::new("variants", "Frappe")
                .label_element(
                    crate::components::button("variant-menu")
                        .debug_selector(|| "variant-menu".into())
                        .label("Frappe")
                        .on_click(move |_, _, _| menu.set(menu.get() + 1))
                        .into_any_element(),
                )
                .on_click(on_card(&self.card)),
        ]))
    }
}

#[gpui::test]
fn plain_label_fires_the_card_and_a_label_element_stays_separate(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_kit::init);
    let (card, menu) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let window = cx.add_window(|_, _| Clicks { card: card.clone(), menu: menu.clone() });
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(*window, cx);
    let click = |visual: &mut VisualTestContext, id: &'static str| {
        let at = visual.debug_bounds(id).unwrap().center();
        visual.simulate_click(at, gpui_kit::Modifiers::default());
    };
    click(&mut visual, "plain");
    assert_eq!(card.get(), 1);
    click(&mut visual, "plain-label");
    assert_eq!(card.get(), 2);
    click(&mut visual, "variant-menu");
    assert_eq!((card.get(), menu.get()), (2, 1));
    click(&mut visual, "variants");
    assert_eq!(card.get(), 3);
    assert_eq!(menu.get(), 1);
}
