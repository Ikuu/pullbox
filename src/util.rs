//! Small helpers shared across the app.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use chrono::Locale;

/// Human readable byte size, e.g. `12.4 MB`.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Local date and time of a Unix timestamp, in the user's locale format
/// (e.g. `09/10/2026 20:58` for en_GB).
pub fn format_date_time(secs: i64) -> String {
    static FORMAT: OnceLock<(Locale, String)> = OnceLock::new();
    let (locale, format) = FORMAT.get_or_init(|| {
        let locale = user_locale();
        (locale, date_time_format(locale))
    });
    match chrono::DateTime::from_timestamp(secs, 0) {
        Some(time) => time.with_timezone(&chrono::Local).format_localized(format, *locale).to_string(),
        None => String::new(),
    }
}

/// The locale's date and time formats, with a four-digit year and no seconds.
fn date_time_format(locale: Locale) -> String {
    let date = pure_rust_locales::locale_match!(locale => LC_TIME::D_FMT).replace("%y", "%Y");
    let time = pure_rust_locales::locale_match!(locale => LC_TIME::T_FMT)
        .replace("%T", "%H:%M")
        .replace("%r", "%I:%M %p")
        .replace(":%S", "");
    format!("{date} {time}")
}

/// The locale for dates: `LC_ALL`, `LC_TIME` or `LANG` (as on Unix), else the
/// system's preferred locale, else POSIX.
fn user_locale() -> Locale {
    let from_env = ["LC_ALL", "LC_TIME", "LANG"]
        .into_iter()
        .find_map(|var| std::env::var(var).ok().filter(|v| !v.is_empty()));
    let Some(name) = from_env.or_else(sys_locale::get_locale) else { return Locale::POSIX };
    // `en_GB.UTF-8@euro` or `en-GB` -> `en_GB`
    let name = name.split(['.', '@']).next().unwrap_or_default().replace('-', "_");
    Locale::try_from(name.as_str()).unwrap_or(Locale::POSIX)
}

/// Current time in seconds since the Unix epoch.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// Standard per-user directories for this app (data, cache, …).
pub fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("", "", "comic-editor")
}

/// Lower-cased file extension, or an empty string.
pub fn lower_ext(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Compare strings the way a human would: `Issue 2` sorts before `Issue 10`.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut a = a.chars().peekable();
    let mut b = b.chars().peekable();
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let na = take_number(&mut a);
                let nb = take_number(&mut b);
                // Compare by magnitude first (ignoring leading zeros), then digits.
                let ta = na.trim_start_matches('0');
                let tb = nb.trim_start_matches('0');
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                a.next();
                b.next();
            }
        }
    }
}

fn take_number(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut digits = String::new();
    while let Some(c) = chars.peek().copied().filter(char::is_ascii_digit) {
        digits.push(c);
        chars.next();
    }
    digits
}

/// Open the system file manager at `dir`.
pub fn open_in_file_manager(dir: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let program = "xdg-open";

    std::process::Command::new(program).arg(dir).spawn().map(|_| ())
}

/// A command running one of the external tools (`unrar`, `avifenc`).
pub fn tool(name: &str) -> Command {
    Command::new(tool_path(name).unwrap_or_else(|| PathBuf::from(name)))
}

/// Where the external tool `name` is: next to our own binary, as in the
/// AppImage, or else on `PATH`. None if it's in neither.
pub fn tool_path(name: &str) -> Option<PathBuf> {
    let bundled = std::env::current_exe().ok().and_then(|exe| Some(exe.parent()?.join(name)));
    let path = std::env::var_os("PATH").unwrap_or_default();
    let on_path = std::env::split_paths(&path).map(|dir| dir.join(name));
    bundled.into_iter().chain(on_path).find(|path| path.is_file())
}

/// Writes a colourful, photo-like JPEG page for tests.
#[cfg(test)]
pub fn write_test_page(path: &Path, width: u32, height: u32) {
    use image::codecs::jpeg::JpegEncoder;
    let page = image::RgbImage::from_fn(width, height, |x, y| {
        let (fx, fy) = (x as f32 / width as f32, y as f32 / height as f32);
        let wave = ((fx * 23.0).sin() * (fy * 17.0).cos() * 0.5 + 0.5) * 255.0;
        image::Rgb([(fx * 255.0) as u8, wave as u8, (fy * 255.0) as u8])
    });
    let file = std::fs::File::create(path).unwrap();
    page.write_with_encoder(JpegEncoder::new_with_quality(file, 95)).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_ordering() {
        assert_eq!(natural_cmp("Issue 2", "Issue 10"), Ordering::Less);
        assert_eq!(natural_cmp("issue 002", "Issue 2"), Ordering::Equal);
        assert_eq!(natural_cmp("a", "B"), Ordering::Less);
    }

    #[test]
    fn date_time_formats() {
        assert_eq!(date_time_format(Locale::en_GB), "%d/%m/%Y %H:%M");
        assert_eq!(date_time_format(Locale::en_US), "%m/%d/%Y %I:%M %p");
        assert_eq!(date_time_format(Locale::de_DE), "%d.%m.%Y %H:%M");
    }

    #[test]
    fn bytes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
    }
}
