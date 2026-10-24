//! Icons from the desktop's icon theme (Breeze on KDE).
//!
//! Breeze icons are drawn in placeholder colours that KDE swaps for the
//! colour scheme's through a `ColorScheme-*` stylesheet; the same is done
//! here before handing the SVG to egui. If the theme or an icon is missing,
//! callers show a text symbol instead.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::{Color32, vec2};

use crate::theme::{self, Ini};

/// Icon size in points, as for small icons in KDE apps.
pub const SIZE: f32 = 16.0;

/// Which text colour an icon is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tone {
    Normal,
    /// On a selection highlight.
    Selected,
}

#[derive(Default)]
struct Cache {
    /// Bumped when the theme changes, so egui doesn't reuse old images.
    generation: u64,
    /// Theme folders, by theme name.
    themes: HashMap<String, Option<ThemeInfo>>,
    /// Recoloured SVGs, by icon name and tone.
    icons: HashMap<(String, Tone), Option<Arc<[u8]>>>,
}

struct ThemeInfo {
    /// Folders holding icons, best size first.
    dirs: Vec<PathBuf>,
    inherits: Vec<String>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

/// Forgets loaded icons, after the icon theme or colour scheme changed.
pub fn reload() {
    if let Ok(mut cache) = CACHE.lock() {
        let generation = cache.as_ref().map_or(0, |c| c.generation + 1);
        *cache = Some(Cache { generation, ..Default::default() });
    }
}

/// The named icon (e.g. "document-edit") at the standard size, if the icon
/// theme has it.
pub fn image(name: &str) -> Option<egui::Image<'static>> {
    toned(name, Tone::Normal)
}

pub fn toned(name: &str, tone: Tone) -> Option<egui::Image<'static>> {
    let mut cache = CACHE.lock().ok()?;
    let cache = cache.get_or_insert_with(Cache::default);
    let key = (name.to_owned(), tone);
    if !cache.icons.contains_key(&key) {
        let svg = find(cache, name)
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|svg| Arc::from(recolor(&svg, tone).into_bytes()));
        cache.icons.insert(key.clone(), svg);
    }
    let bytes = cache.icons.get(&key)?.clone()?;
    let uri = format!("bytes://icons/{}/{name}-{tone:?}.svg", cache.generation);
    Some(egui::Image::from_bytes(uri, bytes).fit_to_exact_size(vec2(SIZE, SIZE)))
}

/// A button with the named icon before `text`, or `fallback` (a symbol
/// character, may be empty) if there's no such icon.
pub fn button<'a>(name: &str, fallback: &str, text: &str) -> egui::Button<'a> {
    button_toned(name, fallback, text, Tone::Normal)
}

pub fn button_toned<'a>(name: &str, fallback: &str, text: &str, tone: Tone) -> egui::Button<'a> {
    match (toned(name, tone), text.is_empty()) {
        (Some(icon), true) => egui::Button::image(icon),
        (Some(icon), false) => egui::Button::image_and_text(icon, text.to_owned()),
        (None, _) if fallback.is_empty() => egui::Button::new(text.to_owned()),
        (None, _) => egui::Button::new(format!("{fallback} {text}").trim().to_owned()),
    }
}

/// Looks the icon up in the icon theme, then the themes it inherits from,
/// then hicolor, as the freedesktop icon theme spec describes.
fn find(cache: &mut Cache, name: &str) -> Option<PathBuf> {
    let mut queue = VecDeque::from([theme::icon_theme(), "hicolor".to_owned()]);
    let mut seen = HashSet::new();
    while let Some(theme_name) = queue.pop_front() {
        if !seen.insert(theme_name.clone()) {
            continue;
        }
        let info = cache.themes.entry(theme_name.clone()).or_insert_with(|| load_theme(&theme_name));
        let Some(info) = info else { continue };
        if let Some(path) = info.dirs.iter().map(|dir| dir.join(format!("{name}.svg"))).find(|p| p.is_file()) {
            return Some(path);
        }
        // Inherited themes come before hicolor, which is always last.
        for (i, parent) in info.inherits.iter().enumerate() {
            queue.insert(i, parent.clone());
        }
    }
    None
}

fn load_theme(name: &str) -> Option<ThemeInfo> {
    let mut dirs: Vec<(u32, PathBuf)> = Vec::new();
    let mut inherits = Vec::new();
    for base in base_dirs() {
        let root = base.join(name);
        let Ok(text) = std::fs::read_to_string(root.join("index.theme")) else { continue };
        let index = Ini::parse(&text);
        for dir in index.get("Icon Theme", "Directories").unwrap_or_default().split(',') {
            let dir = dir.trim();
            if !dir.is_empty() {
                dirs.push((preference(&index, dir), root.join(dir)));
            }
        }
        if inherits.is_empty() {
            inherits = list(index.get("Icon Theme", "Inherits"));
        }
    }
    if dirs.is_empty() {
        return None;
    }
    // Stable, so folders of the same size keep the theme's own order.
    dirs.sort_by_key(|(rank, _)| *rank);
    Some(ThemeInfo { dirs: dirs.into_iter().map(|(_, dir)| dir).collect(), inherits })
}

/// Lower is better: 16px first, then the nearest sizes, then scalable.
fn preference(index: &Ini, dir: &str) -> u32 {
    let size: u32 = index.get(dir, "Size").and_then(|s| s.parse().ok()).unwrap_or(0);
    match (size, index.get(dir, "Type")) {
        (16, _) => 0,
        (22, _) => 1,
        (24, _) => 2,
        (_, Some("Scalable")) => 3,
        (0, _) => 1000,
        (size, _) => 4 + size,
    }
}

fn list(value: Option<&str>) -> Vec<String> {
    value.unwrap_or_default().split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect()
}

/// Where icon themes are installed, in lookup order.
fn base_dirs() -> Vec<PathBuf> {
    let env = |name| std::env::var_os(name).map(PathBuf::from).filter(|p| p.is_absolute());
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let data_home = env("XDG_DATA_HOME").or_else(|| home.as_ref().map(|h| h.join(".local/share")));
    let data_dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
    let data_dirs = if data_dirs.is_empty() { "/usr/local/share:/usr/share".to_owned() } else { data_dirs };

    let mut dirs: Vec<PathBuf> = data_home.map(|d| d.join("icons")).into_iter().collect();
    dirs.extend(home.map(|h| h.join(".icons")));
    dirs.extend(data_dirs.split(':').filter(|d| d.starts_with('/')).map(|d| Path::new(d).join("icons")));
    dirs
}

/// Swaps the colours in a Breeze icon's `ColorScheme-*` stylesheet for the
/// current colour scheme's.
fn recolor(svg: &str, tone: Tone) -> String {
    let palette = theme::palette();
    let text = match tone {
        Tone::Normal => palette.window_fg,
        Tone::Selected => palette.selection_fg,
    };
    let color_for = |role: &str| match role {
        "Text" | "ButtonText" | "ViewText" => Some(text),
        "Background" | "ButtonBackground" | "ViewBackground" => Some(palette.window_bg),
        "Highlight" | "ButtonFocus" | "ButtonHover" | "ViewFocus" | "ViewHover" => Some(palette.focus),
        "HighlightedText" => Some(palette.selection_fg),
        "PositiveText" => Some(palette.positive),
        "NeutralText" => Some(palette.neutral),
        "NegativeText" => Some(palette.negative),
        _ => None,
    };
    recolor_with(svg, color_for)
}

fn recolor_with(svg: &str, color_for: impl Fn(&str) -> Option<Color32>) -> String {
    const CLASS: &str = ".ColorScheme-";
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(start) = rest.find(CLASS) {
        let after = &rest[start + CLASS.len()..];
        let role_len = after.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(after.len());
        let role = &after[..role_len];
        let block = after.find('{').zip(after.find('}')).filter(|(open, close)| open < close);
        let (Some(color), Some((open, close))) = (color_for(role), block) else {
            out.push_str(&rest[..start + CLASS.len()]);
            rest = after;
            continue;
        };
        let body = &after[open + 1..close];
        out.push_str(&rest[..start + CLASS.len()]);
        out.push_str(&after[..open + 1]);
        out.push_str(&set_color(body, color));
        out.push('}');
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

/// Replaces the value of the `color` property in a CSS rule's body.
fn set_color(body: &str, color: Color32) -> String {
    let hex = format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b());
    let mut search = 0;
    while let Some(found) = body[search..].find("color") {
        let at = search + found;
        let is_property = !body[..at].ends_with(|c: char| c == '-' || c.is_ascii_alphanumeric());
        let after = body[at + "color".len()..].trim_start();
        if is_property && after.starts_with(':') {
            let value_start = body.len() - after.len() + 1;
            let value_end = body[value_start..].find(';').map_or(body.len(), |i| value_start + i);
            return format!("{}{hex}{}", &body[..value_start], &body[value_end..]);
        }
        search = at + "color".len();
    }
    body.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recolours_breeze_stylesheets() {
        let svg = r#"<style id="current-color-scheme">.ColorScheme-Text { color: #fcfcfc; } .ColorScheme-Highlight {
            color:#3daee9;
        }</style><path class="ColorScheme-Text" style="fill:currentColor"/>"#;
        let out = recolor_with(svg, |role| match role {
            "Text" => Some(Color32::from_rgb(1, 2, 3)),
            _ => None,
        });
        assert!(out.contains(".ColorScheme-Text { color:#010203; }"), "{out}");
        assert!(out.contains("color:#3daee9;"), "{out}");
        assert!(out.ends_with(r#"<path class="ColorScheme-Text" style="fill:currentColor"/>"#));
    }

    #[test]
    fn leaves_other_colour_properties_alone() {
        assert_eq!(set_color(" background-color: red; ", Color32::BLACK), " background-color: red; ");
        assert_eq!(set_color("color:red", Color32::BLACK), "color:#000000");
    }
}
