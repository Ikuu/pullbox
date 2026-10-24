//! Colours and fonts. egui draws its own widgets, so it can't use the Qt
//! style; instead the desktop's colour scheme, font and icon theme are read
//! from KDE's `kdeglobals` and turned into an egui style. Without KDE
//! settings, Breeze Dark's colours and the fontconfig default font are used.
//! `kdeglobals` is watched, so changing the scheme restyles the app.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, vec2};

/// Font family for bold text: egui has no font weights, so the bold face is
/// loaded as a family of its own.
const BOLD: &str = "bold";

/// KDE's default UI font size, in points.
const DEFAULT_POINTS: f32 = 10.0;

/// Colours from the desktop's colour scheme, named after KDE's colour roles.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub window_bg: Color32,
    pub window_fg: Color32,
    pub window_fg_inactive: Color32,
    pub view_bg: Color32,
    pub view_bg_alt: Color32,
    pub header_bg: Color32,
    pub button_bg: Color32,
    pub button_fg: Color32,
    pub selection_bg: Color32,
    pub selection_fg: Color32,
    pub focus: Color32,
    pub hover: Color32,
    pub link: Color32,
    pub positive: Color32,
    pub neutral: Color32,
    pub negative: Color32,
}

impl Palette {
    /// KDE's default dark scheme.
    const BREEZE_DARK: Self = Self {
        window_bg: Color32::from_rgb(32, 35, 38),
        window_fg: Color32::from_rgb(252, 252, 252),
        window_fg_inactive: Color32::from_rgb(161, 169, 177),
        view_bg: Color32::from_rgb(20, 22, 24),
        view_bg_alt: Color32::from_rgb(29, 31, 34),
        header_bg: Color32::from_rgb(41, 44, 48),
        button_bg: Color32::from_rgb(41, 44, 48),
        button_fg: Color32::from_rgb(252, 252, 252),
        selection_bg: Color32::from_rgb(61, 174, 233),
        selection_fg: Color32::from_rgb(252, 252, 252),
        focus: Color32::from_rgb(61, 174, 233),
        hover: Color32::from_rgb(61, 174, 233),
        link: Color32::from_rgb(29, 153, 243),
        positive: Color32::from_rgb(39, 174, 96),
        neutral: Color32::from_rgb(246, 116, 0),
        negative: Color32::from_rgb(218, 68, 83),
    };

    fn from_kde(kde: &Ini) -> Option<Self> {
        let get = |group: &str, key: &str| kde.get(&format!("Colors:{group}"), key).and_then(parse_color);
        let fallback = Self::BREEZE_DARK;
        let window_bg = get("Window", "BackgroundNormal")?;
        Some(Self {
            window_bg,
            window_fg: get("Window", "ForegroundNormal").unwrap_or(fallback.window_fg),
            window_fg_inactive: get("Window", "ForegroundInactive").unwrap_or(fallback.window_fg_inactive),
            view_bg: get("View", "BackgroundNormal").unwrap_or(window_bg),
            view_bg_alt: get("View", "BackgroundAlternate").unwrap_or(window_bg),
            // Older schemes have no Header group; their headers use the window colours.
            header_bg: get("Header", "BackgroundNormal").unwrap_or(window_bg),
            button_bg: get("Button", "BackgroundNormal").unwrap_or(fallback.button_bg),
            button_fg: get("Button", "ForegroundNormal").unwrap_or(fallback.button_fg),
            selection_bg: get("Selection", "BackgroundNormal").unwrap_or(fallback.selection_bg),
            selection_fg: get("Selection", "ForegroundNormal").unwrap_or(fallback.selection_fg),
            focus: get("Button", "DecorationFocus").unwrap_or(fallback.focus),
            hover: get("Button", "DecorationHover").unwrap_or(fallback.hover),
            link: get("View", "ForegroundLink").unwrap_or(fallback.link),
            positive: get("View", "ForegroundPositive").unwrap_or(fallback.positive),
            neutral: get("View", "ForegroundNeutral").unwrap_or(fallback.neutral),
            negative: get("View", "ForegroundNegative").unwrap_or(fallback.negative),
        })
    }

    fn is_dark(&self) -> bool {
        let [r, g, b, _] = self.window_bg.to_array();
        (u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000 < 128
    }
}

/// KDE's default icon theme. Its icons are recoloured to suit the colour
/// scheme, so it works for dark and light schemes alike.
const DEFAULT_ICON_THEME: &str = "breeze";

struct Current {
    palette: Palette,
    icon_theme: String,
}

static CURRENT: RwLock<Option<Current>> = RwLock::new(None);
/// Set by the watcher thread when `kdeglobals` changes.
static CHANGED: AtomicBool = AtomicBool::new(false);

/// The colours in use, for things the egui style has no slot for.
pub fn palette() -> Palette {
    CURRENT.read().ok().and_then(|c| c.as_ref().map(|c| c.palette)).unwrap_or(Palette::BREEZE_DARK)
}

/// Name of the desktop's icon theme, e.g. "breeze-dark".
pub fn icon_theme() -> String {
    CURRENT
        .read()
        .ok()
        .and_then(|c| c.as_ref().map(|c| c.icon_theme.clone()))
        .unwrap_or_else(|| DEFAULT_ICON_THEME.to_owned())
}

/// Font family for bold text.
pub fn bold() -> FontFamily {
    FontFamily::Name(BOLD.into())
}

/// Sets up colours, fonts and spacing, and starts watching for changes to
/// the desktop's settings. Call once at startup.
pub fn init(ctx: &egui::Context) {
    apply(ctx);
    watch(ctx);
}

/// Restyles the app if the desktop's settings changed. Call every frame.
pub fn reload_if_changed(ctx: &egui::Context) {
    if CHANGED.swap(false, Ordering::Relaxed) {
        apply(ctx);
        crate::icons::reload();
        // Drops the old icon images; they're cached by URI.
        ctx.forget_all_images();
    }
}

fn apply(ctx: &egui::Context) {
    let kde = kdeglobals_path().and_then(|path| std::fs::read_to_string(path).ok()).map(|text| Ini::parse(&text));
    let palette = kde.as_ref().and_then(Palette::from_kde).unwrap_or(Palette::BREEZE_DARK);
    let icon_theme = kde.as_ref().and_then(|k| k.get("Icons", "Theme")).unwrap_or(DEFAULT_ICON_THEME).to_owned();
    if let Ok(mut current) = CURRENT.write() {
        *current = Some(Current { palette, icon_theme });
    }

    let ui_font = kde.as_ref().and_then(|k| FontSpec::parse(k.get("General", "font")?));
    let ui_font = ui_font.unwrap_or_else(|| FontSpec { family: "sans-serif".into(), points: DEFAULT_POINTS });
    let mono_font = kde.as_ref().and_then(|k| FontSpec::parse(k.get("General", "fixed")?));
    let mono_font = mono_font.unwrap_or_else(|| FontSpec { family: "monospace".into(), points: ui_font.points });
    ctx.set_fonts(fonts(&ui_font, &mono_font));

    let theme = if palette.is_dark() { egui::Theme::Dark } else { egui::Theme::Light };
    ctx.set_theme(theme);
    ctx.set_style_of(theme, style(&palette, theme, ui_font.pixels(), mono_font.pixels()));
}

fn style(p: &Palette, theme: egui::Theme, body: f32, mono: f32) -> egui::Style {
    let mut style = theme.default_style();

    style.text_styles = [
        (TextStyle::Small, FontId::proportional((body * 0.85).round())),
        (TextStyle::Body, FontId::proportional(body)),
        (TextStyle::Button, FontId::proportional(body)),
        (TextStyle::Monospace, FontId::monospace(mono)),
        (TextStyle::Heading, FontId::new((body * 1.35).round(), bold())),
    ]
    .into();

    let spacing = &mut style.spacing;
    spacing.item_spacing = vec2(8.0, 5.0);
    spacing.button_padding = vec2(10.0, 4.0);
    spacing.interact_size.y = (body + 10.0).round();
    spacing.window_margin = Margin::same(14);
    spacing.menu_margin = Margin::same(6);
    spacing.indent = 16.0;

    let v = &mut style.visuals;
    let line = mix(p.window_bg, p.window_fg, 0.15);
    let radius = CornerRadius::same(5);

    v.weak_text_color = Some(p.window_fg_inactive);
    v.hyperlink_color = p.link;
    v.faint_bg_color = p.view_bg_alt;
    v.extreme_bg_color = p.view_bg;
    v.code_bg_color = p.view_bg_alt;
    v.warn_fg_color = p.neutral;
    v.error_fg_color = p.negative;
    v.panel_fill = p.window_bg;
    v.window_fill = p.window_bg;
    v.window_stroke = Stroke::new(1.0, line);
    v.window_corner_radius = CornerRadius::same(8);
    v.window_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(110) };
    v.popup_shadow = Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(80) };
    v.menu_corner_radius = CornerRadius::same(6);
    v.selection.bg_fill = p.selection_bg;
    v.selection.stroke = Stroke::new(1.0, p.selection_fg);
    v.text_cursor.stroke = Stroke::new(2.0, p.focus);
    v.indent_has_left_vline = false;
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.window_bg;
    w.noninteractive.weak_bg_fill = p.window_bg;
    w.noninteractive.bg_stroke = Stroke::new(1.0, line);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.window_fg);
    w.noninteractive.corner_radius = radius;

    w.inactive.bg_fill = p.button_bg;
    w.inactive.weak_bg_fill = p.button_bg;
    w.inactive.bg_stroke = Stroke::new(1.0, mix(p.button_bg, p.button_fg, 0.1));
    w.inactive.fg_stroke = Stroke::new(1.0, p.button_fg);
    w.inactive.corner_radius = radius;

    let hovered = mix(p.button_bg, p.hover, 0.2);
    w.hovered.bg_fill = hovered;
    w.hovered.weak_bg_fill = hovered;
    w.hovered.bg_stroke = Stroke::new(1.0, p.hover);
    w.hovered.fg_stroke = Stroke::new(1.5, p.button_fg);
    w.hovered.corner_radius = radius;
    w.hovered.expansion = 0.0;

    let active = mix(p.button_bg, p.focus, 0.35);
    w.active.bg_fill = active;
    w.active.weak_bg_fill = active;
    w.active.bg_stroke = Stroke::new(1.0, p.focus);
    w.active.fg_stroke = Stroke::new(1.5, p.button_fg);
    w.active.corner_radius = radius;
    w.active.expansion = 0.0;

    w.open.bg_fill = p.window_bg;
    w.open.weak_bg_fill = mix(p.button_bg, p.focus, 0.2);
    w.open.bg_stroke = Stroke::new(1.0, line);
    w.open.fg_stroke = Stroke::new(1.0, p.button_fg);
    w.open.corner_radius = radius;

    style
}

/// `a` moved towards `b` by `t` (0 to 1).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let channel = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    Color32::from_rgb(channel(a.r(), b.r()), channel(a.g(), b.g()), channel(a.b(), b.b()))
}

/// "r,g,b" or "r,g,b,a", as written in `kdeglobals`.
fn parse_color(value: &str) -> Option<Color32> {
    let mut parts = value.split(',').map(|p| p.trim().parse::<u8>());
    let (r, g, b) = (parts.next()?.ok()?, parts.next()?.ok()?, parts.next()?.ok()?);
    Some(Color32::from_rgb(r, g, b))
}

// ----- fonts -----

struct FontSpec {
    family: String,
    points: f32,
}

impl FontSpec {
    /// A Qt font string as stored in `kdeglobals`: "Noto Sans,10,-1,5,400,0,0,0,0,0".
    fn parse(value: &str) -> Option<Self> {
        let mut parts = value.split(',');
        let family = parts.next()?.trim().to_owned();
        // -1 means the size is in pixels instead; the default size is fine then.
        let points = parts.next().and_then(|p| p.trim().parse::<f32>().ok()).filter(|&p| p > 0.0);
        (!family.is_empty()).then(|| Self { family, points: points.unwrap_or(DEFAULT_POINTS) })
    }

    /// Size in egui points, which are 1/96 inch like Qt's logical pixels.
    fn pixels(&self) -> f32 {
        (self.points * 96.0 / 72.0).round()
    }
}

/// egui's own fonts stay as fallbacks: they have the symbols and emoji used
/// in labels, which desktop text fonts often lack.
fn fonts(ui_font: &FontSpec, mono_font: &FontSpec) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();

    let mut add = |name: &str, pattern: &str, family: FontFamily| {
        if let Some(data) = system_font(pattern) {
            fonts.font_data.insert(name.to_owned(), data.into());
            fonts.families.entry(family).or_default().insert(0, name.to_owned());
        }
    };
    add("system-ui", &ui_font.family, FontFamily::Proportional);
    add("system-mono", &mono_font.family, FontFamily::Monospace);

    // Falls back to the regular face, so the family always exists.
    let mut bold_family = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    if let Some(data) = system_font(&format!("{}:bold", ui_font.family)) {
        fonts.font_data.insert("system-ui-bold".into(), data.into());
        bold_family.insert(0, "system-ui-bold".into());
    }
    fonts.families.insert(bold(), bold_family);
    fonts
}

/// Loads the font fontconfig picks for `pattern`, e.g. "Noto Sans:bold".
fn system_font(pattern: &str) -> Option<FontData> {
    let output = Command::new("fc-match").args(["--format", "%{file}\n%{index}", pattern]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let output = String::from_utf8(output.stdout).ok()?;
    let mut lines = output.lines();
    let file = lines.next().filter(|f| !f.is_empty())?;
    let index = lines.next().and_then(|i| i.trim().parse().ok()).unwrap_or(0);
    let bytes = std::fs::read(file).ok()?;
    let mut data = FontData::from_owned(bytes);
    data.index = index;
    Some(data)
}

// ----- kdeglobals -----

/// A parsed INI file: values by group, then key.
pub struct Ini(HashMap<String, HashMap<String, String>>);

impl Ini {
    pub fn parse(text: &str) -> Self {
        let mut groups: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut group = String::new();
        for line in text.lines().map(str::trim) {
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                group = name.to_owned();
            } else if let Some((key, value)) = line.split_once('=') {
                groups.entry(group.clone()).or_default().insert(key.trim().to_owned(), value.trim().to_owned());
            }
        }
        Self(groups)
    }

    pub fn get(&self, group: &str, key: &str) -> Option<&str> {
        self.0.get(group)?.get(key).map(String::as_str).filter(|v| !v.is_empty())
    }
}

fn kdeglobals_path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("kdeglobals"))
}

/// Checks `kdeglobals` every couple of seconds on a background thread, so an
/// idle app doesn't have to keep redrawing to notice a change.
fn watch(ctx: &egui::Context) {
    let Some(path) = kdeglobals_path() else { return };
    let modified = move || std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let mut last: Option<SystemTime> = modified();
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let now = modified();
            if now != last {
                last = now;
                CHANGED.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_kde_colours() {
        let ini = Ini::parse("[Colors:Window]\nBackgroundNormal=30, 32, 48\n\n[Colors:Selection]\nBackgroundNormal=138,173,244,255\n");
        let palette = Palette::from_kde(&ini).unwrap();
        assert_eq!(palette.window_bg, Color32::from_rgb(30, 32, 48));
        assert_eq!(palette.selection_bg, Color32::from_rgb(138, 173, 244));
        assert_eq!(palette.header_bg, palette.window_bg);
        assert!(palette.is_dark());
    }

    #[test]
    fn no_window_colours_means_no_scheme() {
        assert!(Palette::from_kde(&Ini::parse("[General]\nfont=Noto Sans,10\n")).is_none());
    }

    #[test]
    fn parses_qt_font_strings() {
        let font = FontSpec::parse("Noto Sans,11,-1,5,400,0,0,0,0,0").unwrap();
        assert_eq!((font.family.as_str(), font.points), ("Noto Sans", 11.0));
        assert_eq!(FontSpec::parse("Hack,-1,14,5").unwrap().points, DEFAULT_POINTS);
        assert!(FontSpec::parse("").is_none());
    }
}
