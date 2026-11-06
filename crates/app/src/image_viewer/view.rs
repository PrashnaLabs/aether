use super::{ImageViewer, data};
use aether_ui::theme::Corners;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui_kit::rems;
use gpui_kit::{
    Bounds, Context, Keystroke, MouseButton, Render, Window, canvas, div, fill, img, point,
    prelude::*, px, size,
};

impl ImageViewer {
    fn area(&self, window: &Window) -> [f32; 2] {
        let size = window.viewport_size();
        [(f32::from(size.width) - 48.).max(1.), (f32::from(size.height) - 104.).max(1.)]
    }

    fn scale(&self, window: &Window) -> f32 {
        self.zoom.unwrap_or_else(|| {
            self.loaded.as_ref().map_or(1., |image| {
                data::fit_scale(image.dimensions.map(|v| v as f32), self.area(window))
            })
        })
    }

    fn change_zoom(&mut self, factor: f32, window: &Window, cx: &mut Context<Self>) {
        self.zoom = Some((self.scale(window) * factor).clamp(0.01, 16.));
        cx.notify();
    }

    fn fit(&mut self, cx: &mut Context<Self>) {
        self.zoom = None;
        self.pan = [0.; 2];
        cx.notify();
    }

    pub(super) fn handle_key(
        &mut self,
        event: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.key.as_str() {
            "escape" => self.close(window, cx),
            "+" | "=" => self.change_zoom(1.25, window, cx),
            "-" => self.change_zoom(0.8, window, cx),
            "0" => self.fit(cx),
            "1" => {
                self.zoom = Some(1.);
                self.pan = [0.; 2];
                cx.notify();
            }
            _ => {}
        }
        cx.stop_propagation();
        window.prevent_default();
    }
}

impl Render for ImageViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg, muted, border) =
            (theme.background.alpha(1.), theme.foreground, theme.muted_foreground, theme.border);
        let area = self.area(window);
        let scale = self.scale(window);
        let title =
            self.path.file_name().unwrap_or(self.path.as_os_str()).to_string_lossy().into_owned();
        let dimensions = self.loaded.as_ref().map(|image| {
            format!("{} × {} · {:.0}%", image.dimensions[0], image.dimensions[1], scale * 100.)
        });
        let message = self.error.clone().unwrap_or_else(|| "Loading image…".into());
        let mut body = div()
            .id("image-viewport")
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &gpui_kit::MouseDownEvent, window, cx| {
                    this.focus.focus(window, cx);
                    this.drag =
                        Some(([f32::from(ev.position.x), f32::from(ev.position.y)], this.pan));
                    cx.stop_propagation();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.drag = None;
                    cx.stop_propagation();
                }),
            )
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, _| this.drag = None))
            .on_mouse_move(cx.listener(|this, ev: &gpui_kit::MouseMoveEvent, _, cx| {
                if !ev.dragging() {
                    this.drag = None;
                    return;
                }
                if let Some((origin, pan)) = this.drag {
                    this.pan = [
                        pan[0] + f32::from(ev.position.x) - origin[0],
                        pan[1] + f32::from(ev.position.y) - origin[1],
                    ];
                    cx.notify();
                }
                cx.stop_propagation();
            }))
            .on_scroll_wheel(cx.listener(|this, ev: &gpui_kit::ScrollWheelEvent, window, cx| {
                let delta = ev.delta.pixel_delta(px(20.));
                this.change_zoom((f32::from(delta.y) * 0.002).clamp(-0.25, 0.25).exp(), window, cx);
                cx.stop_propagation();
            }))
            .on_pinch(cx.listener(|this, ev: &gpui_kit::PinchEvent, window, cx| {
                this.change_zoom((1. + ev.delta).max(0.1), window, cx);
                cx.stop_propagation();
            }));
        if let Some(image) = &self.loaded {
            let rendered = image.dimensions.map(|v| v as f32 * scale);
            self.pan = data::clamp_pan(self.pan, rendered, area);
            let pan = self.pan;
            let light = bg.blend(fg.opacity(0.045));
            let dark = bg.blend(fg.opacity(0.09));
            body = body
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, window, _| {
                            let cols = (f32::from(bounds.size.width) / 16.).ceil() as u32;
                            let rows = (f32::from(bounds.size.height) / 16.).ceil() as u32;
                            for y in 0..rows {
                                for x in 0..cols {
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            point(
                                                bounds.origin.x + px(x as f32 * 16.),
                                                bounds.origin.y + px(y as f32 * 16.),
                                            ),
                                            size(px(16.), px(16.)),
                                        ),
                                        if (x + y) % 2 == 0 { light } else { dark },
                                    ));
                                }
                            }
                        },
                    )
                    .absolute()
                    .inset_0(),
                )
                .child(
                    img(image.pixels.clone())
                        .absolute()
                        .left(px((area[0] - rendered[0]) / 2. + pan[0]))
                        .top(px((area[1] - rendered[1]) / 2. + pan[1]))
                        .w(px(rendered[0]))
                        .h(px(rendered[1])),
                );
        } else {
            body = body.flex().items_center().justify_center().text_color(muted).child(message);
        }
        div()
            .id("image-viewer-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .bg(gpui_kit::black().opacity(0.65))
            .p(px(24.))
            .track_focus(&self.focus)
            .key_context("ImageViewer")
            .on_action(|_: &crate::actions::StopGenerating, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .child(
                v_flex()
                    .size_full()
                    .min_h_0()
                    .corner(rems(0.5))
                    .overflow_hidden()
                    .bg(bg)
                    .border_1()
                    .border_color(border)
                    .child(
                        h_flex()
                            .h(px(56.))
                            .px_3()
                            .gap_2()
                            .items_center()
                            .flex_none()
                            .text_color(fg)
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(div().truncate().child(title))
                                    .children(dimensions.map(|label| {
                                        div().truncate().text_xs().text_color(muted).child(label)
                                    })),
                            )
                            .child(
                                aether_ui::button("image-zoom-out")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Minus)
                                    .tooltip("Zoom out (-)")
                                    .accessibility_label("Zoom out")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.change_zoom(0.8, window, cx)
                                    })),
                            )
                            .child(
                                aether_ui::button("image-zoom-in")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Plus)
                                    .tooltip("Zoom in (+)")
                                    .accessibility_label("Zoom in")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.change_zoom(1.25, window, cx)
                                    })),
                            )
                            .child(
                                aether_ui::button("image-actual")
                                    .ghost()
                                    .small()
                                    .label("1:1")
                                    .tooltip("Actual size (1)")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.zoom = Some(1.);
                                        this.pan = [0.; 2];
                                        cx.notify();
                                    })),
                            )
                            .child(
                                aether_ui::button("image-fit")
                                    .ghost()
                                    .small()
                                    .label("Fit")
                                    .tooltip("Fit to window (0)")
                                    .on_click(cx.listener(|this, _, _, cx| this.fit(cx))),
                            )
                            .child(
                                aether_ui::button("image-close")
                                    .ghost()
                                    .small()
                                    .icon(IconName::X)
                                    .tooltip("Close image (Esc)")
                                    .accessibility_label("Close image")
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.close(window, cx)),
                                    ),
                            ),
                    )
                    .child(body),
            )
    }
}
