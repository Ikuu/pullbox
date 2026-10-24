//! Short notices in the bottom-right corner, e.g. when a job finishes. They
//! fade after a few seconds; clicking one dismisses it.

use egui::{Align2, Color32, Frame, Margin, Sense, Stroke, vec2};

use crate::icons;
use crate::theme;

const SHOWN_FOR: f64 = 5.0;
/// Errors stay longer: they're worth reading.
const ERRORS_SHOWN_FOR: f64 = 10.0;
const FADE: f64 = 0.4;
const MAX_SHOWN: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

impl ToastKind {
    fn icon(self) -> (&'static str, &'static str) {
        match self {
            ToastKind::Info => ("dialog-information", "ℹ"),
            ToastKind::Success => ("dialog-positive", "✔"),
            ToastKind::Warning => ("dialog-warning", "⚠"),
            ToastKind::Error => ("dialog-error", "✖"),
        }
    }

    fn color(self) -> Color32 {
        let palette = theme::palette();
        match self {
            ToastKind::Info => palette.focus,
            ToastKind::Success => palette.positive,
            ToastKind::Warning => palette.neutral,
            ToastKind::Error => palette.negative,
        }
    }

    fn lifetime(self) -> f64 {
        if self == ToastKind::Error { ERRORS_SHOWN_FOR } else { SHOWN_FOR }
    }
}

struct Toast {
    kind: ToastKind,
    text: String,
    /// When it was first shown (egui time); set on the first frame.
    shown_at: Option<f64>,
}

#[derive(Default)]
pub struct Toasts {
    toasts: Vec<Toast>,
}

impl Toasts {
    pub fn push(&mut self, kind: ToastKind, text: impl Into<String>) {
        let text = text.into();
        // Repeating the newest toast just restarts it.
        self.toasts.retain(|t| t.text != text);
        self.toasts.push(Toast { kind, text, shown_at: None });
        if self.toasts.len() > MAX_SHOWN {
            self.toasts.remove(0);
        }
    }

    /// Draws the toasts above the status bar, newest at the bottom.
    pub fn show(&mut self, ctx: &egui::Context, bottom_margin: f32) {
        let now = ctx.input(|i| i.time);
        self.toasts.retain(|t| t.shown_at.is_none_or(|at| now - at < t.kind.lifetime()));
        if self.toasts.is_empty() {
            return;
        }

        let mut dismissed = None;
        // Seconds until something needs redrawing: a fade starting or running.
        let mut wake = f64::INFINITY;
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(Align2::RIGHT_BOTTOM, vec2(-14.0, -bottom_margin - 10.0))
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                for (i, toast) in self.toasts.iter_mut().enumerate() {
                    let shown_at = *toast.shown_at.get_or_insert(now);
                    let left = toast.kind.lifetime() - (now - shown_at);
                    let opacity = (left / FADE).clamp(0.0, 1.0) as f32;
                    wake = wake.min(if left > FADE { left - FADE } else { 1.0 / 30.0 });
                    ui.set_opacity(opacity);
                    if show_toast(ui, toast).clicked() {
                        dismissed = Some(i);
                    }
                }
            });
        if let Some(i) = dismissed {
            self.toasts.remove(i);
        }

        // Fade and expire them even if nothing else happens.
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(wake));
    }
}

fn show_toast(ui: &mut egui::Ui, toast: &Toast) -> egui::Response {
    let visuals = ui.visuals();
    let accent = toast.kind.color();
    let frame = Frame::new()
        .fill(visuals.window_fill)
        .stroke(Stroke::new(1.0, theme::mix(visuals.window_fill, accent, 0.6)))
        .corner_radius(visuals.window_corner_radius)
        .shadow(visuals.popup_shadow)
        .inner_margin(Margin::symmetric(12, 10));
    let response = frame
        .show(ui, |ui| {
            ui.set_max_width(380.0);
            ui.horizontal(|ui| {
                let (name, fallback) = toast.kind.icon();
                match icons::image(name) {
                    Some(icon) => ui.add(icon),
                    None => ui.colored_label(accent, fallback),
                };
                ui.add(egui::Label::new(&toast.text).wrap());
            });
        })
        .response;
    response.interact(Sense::click()).on_hover_text("Click to dismiss")
}
