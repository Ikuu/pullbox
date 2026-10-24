//! The Library Stats window: sizes, compression savings and tagging, for the
//! whole library or one folder.

use egui::{CornerRadius, Rect, RichText, Sense, vec2};

use super::widgets::section;
use crate::library::stats::{Group, Stats};
use crate::theme;
use crate::util::format_bytes;

/// Series and publishers listed before "Show all".
const TOP: usize = 10;
const BAR_HEIGHT: f32 = 10.0;
const ROW_HEIGHT: f32 = 22.0;
const NAME_WIDTH: f32 = 220.0;
const VALUE_WIDTH: f32 = 190.0;

pub struct StatsDialog {
    /// What the stats cover, e.g. "the whole library".
    scope: String,
    stats: Stats,
    recent_days: u32,
    all_series: bool,
    all_publishers: bool,
}

impl StatsDialog {
    pub fn new(scope: String, stats: Stats, recent_days: u32) -> Self {
        Self { scope, stats, recent_days, all_series: false, all_publishers: false }
    }

    /// Returns false once the user closes the window.
    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        let mut open = true;
        egui::Window::new("Library Stats").open(&mut open).default_size([760.0, 640.0]).collapsible(false).show(
            ctx,
            |ui| {
                ui.weak(format!("Stats for {}", self.scope));
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| self.body(ui));
            },
        );
        open
    }

    fn body(&mut self, ui: &mut egui::Ui) {
        let s = &self.stats;
        ui.add_space(8.0);
        self.tiles(ui);
        ui.add_space(6.0);
        ui.weak(format!(
            "{} series · {} pages · {} added in the last {} days",
            count(s.series as u64),
            count(s.pages),
            count(s.recent as u64),
            self.recent_days
        ));

        section(ui, "Compression");
        meter(ui, s.compressed, s.comics, "compressed");
        ui.label(format!(
            "{} uncompressed comics take {}.",
            count((s.comics - s.compressed) as u64),
            format_bytes(s.uncompressed_size)
        ));
        if s.compressed_unrecorded > 0 {
            ui.weak(format!(
                "{} comics were compressed before sizes were recorded, so their savings aren't included.",
                count(s.compressed_unrecorded as u64)
            ));
        }

        section(ui, "Tagging");
        meter(ui, s.comics - s.untagged, s.comics, "tagged");
        if s.untagged > 0 {
            ui.label(format!("Untagged comics take {}.", format_bytes(s.untagged_size)));
        }

        section(ui, "Formats");
        bar_list(ui, "formats", &s.formats, s.size, s.formats.len());

        section(ui, "Largest series");
        let shown = if self.all_series { s.series_by_size.len() } else { TOP };
        bar_list(ui, "series", &s.series_by_size, s.size, shown);
        show_all_toggle(ui, &mut self.all_series, s.series_by_size.len());

        section(ui, "Publishers");
        let shown = if self.all_publishers { s.publishers.len() } else { TOP };
        bar_list(ui, "publishers", &s.publishers, s.size, shown);
        show_all_toggle(ui, &mut self.all_publishers, s.publishers.len());
        ui.add_space(8.0);
    }

    /// The headline numbers, as a row of tiles.
    fn tiles(&self, ui: &mut egui::Ui) {
        let s = &self.stats;
        let saved = if s.recorded == 0 {
            ("—".to_owned(), "Not recorded yet: savings are recorded for comics compressed from now on".to_owned())
        } else {
            let smaller = percent(s.saved(), s.recorded_before);
            (format_bytes(s.saved()), format!("{smaller} smaller across {} comics", count(s.recorded as u64)))
        };
        let average = if s.comics == 0 { 0 } else { s.size / s.comics as u64 };
        let tiles = [
            ("Comics", count(s.comics as u64), format!("{} series", count(s.series as u64))),
            ("Total size", format_bytes(s.size), format!("{} per comic on average", format_bytes(average))),
            ("Space saved", saved.0, saved.1),
            (
                "Untagged",
                count(s.untagged as u64),
                format!("{} of comics, {}", percent(s.untagged as u64, s.comics as u64), format_bytes(s.untagged_size)),
            ),
        ];

        let gap = ui.spacing().item_spacing.x;
        let width = ((ui.available_width() - gap * 3.0) / 4.0).max(140.0);
        ui.horizontal_top(|ui| {
            for (label, value, detail) in tiles {
                stat_tile(ui, width, label, &value, &detail);
            }
        });
    }
}

fn stat_tile(ui: &mut egui::Ui, width: f32, label: &str, value: &str, detail: &str) {
    let visuals = ui.visuals();
    egui::Frame::new()
        .fill(theme::palette().view_bg)
        .stroke(visuals.widgets.noninteractive.bg_stroke)
        .corner_radius(6)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                // The frame's margins and stroke are outside this width.
                ui.set_width(width - 26.0);
                ui.set_min_height(72.0);
                ui.weak(label);
                ui.label(RichText::new(value).size(22.0).family(theme::bold()));
                ui.add(egui::Label::new(RichText::new(detail).small().weak()).wrap());
            });
        });
}

/// "2,980 of 3,603 comics compressed (83%)" over a bar showing the share.
fn meter(ui: &mut egui::Ui, part: usize, whole: usize, what: &str) {
    let (part, whole) = (part as u64, whole as u64);
    ui.label(format!("{} of {} comics {what} ({})", count(part), count(whole), percent(part, whole)));
    let width = ui.available_width().min(520.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, BAR_HEIGHT), Sense::hover());
    let accent = theme::palette().focus;
    let radius = CornerRadius::same((BAR_HEIGHT / 2.0) as u8);
    // The track is a lighter step of the fill's own hue.
    ui.painter().rect_filled(rect, radius, theme::mix(theme::palette().view_bg, accent, 0.25));
    if whole > 0 && part > 0 {
        let filled = Rect::from_min_size(rect.min, vec2(rect.width() * part as f32 / whole as f32, rect.height()));
        ui.painter().rect_filled(filled, radius, accent);
    }
    ui.add_space(4.0);
}

/// One row per group: name, a bar for its size, then size and comic count.
/// Bars are scaled to the largest group and share one baseline.
fn bar_list(ui: &mut egui::Ui, id: &str, groups: &[Group], total: u64, shown: usize) {
    let largest = groups.first().map_or(1, |g| g.size.max(1));
    let accent = theme::palette().focus;
    for (i, group) in groups.iter().take(shown).enumerate() {
        let row = ui
            .horizontal(|ui| {
                ui.set_height(ROW_HEIGHT);
                let left = egui::Layout::left_to_right(egui::Align::Center);
                ui.allocate_ui_with_layout(vec2(NAME_WIDTH, ROW_HEIGHT), left, |ui| {
                    ui.set_width(NAME_WIDTH);
                    ui.add(egui::Label::new(&group.name).truncate());
                });
                let bar_width = (ui.available_width() - VALUE_WIDTH).max(60.0);
                let (area, _) = ui.allocate_exact_size(vec2(bar_width, ROW_HEIGHT), Sense::hover());
                let length = (area.width() * group.size as f32 / largest as f32).max(2.0);
                let bar =
                    Rect::from_min_size(area.left_center() - vec2(0.0, BAR_HEIGHT / 2.0), vec2(length, BAR_HEIGHT));
                // Rounded at the data end, square at the baseline.
                let corners = CornerRadius { nw: 0, sw: 0, ne: 4, se: 4 };
                ui.painter().rect_filled(bar, corners, accent);
                ui.label(format_bytes(group.size));
                ui.weak(format!("· {} comics", count(group.comics as u64)));
            })
            .response;
        let tooltip = format!(
            "{}\n{} ({} of the total)\n{} comics",
            group.name,
            format_bytes(group.size),
            percent(group.size, total),
            count(group.comics as u64)
        );
        ui.interact(row.rect, ui.id().with((id, i)), Sense::hover()).on_hover_text(tooltip);
    }
    if groups.is_empty() {
        ui.weak("None");
    }
}

fn show_all_toggle(ui: &mut egui::Ui, all: &mut bool, len: usize) {
    if len <= TOP {
        return;
    }
    let text = if *all { "Show fewer".to_owned() } else { format!("Show all {len}") };
    if ui.small_button(text).clicked() {
        *all = !*all;
    }
}

/// 3603 → "3,603".
fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Whole percent, never rounded to 0% or 100% unless it is: "<1%", ">99%".
fn percent(part: u64, whole: u64) -> String {
    if whole == 0 {
        return "0%".to_owned();
    }
    let value = part as f64 / whole as f64 * 100.0;
    match value {
        v if part > 0 && v < 1.0 => "<1%".to_owned(),
        v if part < whole && v > 99.0 => ">99%".to_owned(),
        v => format!("{v:.0}%"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_counts_and_percentages() {
        assert_eq!(count(0), "0");
        assert_eq!(count(999), "999");
        assert_eq!(count(3603), "3,603");
        assert_eq!(count(1_234_567), "1,234,567");
        assert_eq!(percent(1, 3), "33%");
        assert_eq!(percent(1, 1000), "<1%");
        assert_eq!(percent(3594, 3603), ">99%");
        assert_eq!(percent(5, 5), "100%");
        assert_eq!(percent(0, 0), "0%");
    }
}
