//! CBR (rar) archives, read and extracted through the `unrar` command line tool.

use std::ffi::OsStr;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::{Pages, cover_entry, is_comicinfo_entry};
use crate::util::tool;

/// Returns the image counts and the raw ComicInfo.xml, if any.
pub(super) fn read_summary(path: &Path) -> Result<(Pages, Option<String>)> {
    let entries = list_entries(path)?;

    let pages = Pages::count(entries.iter().map(String::as_str));
    let xml = match entries.iter().find(|e| is_comicinfo_entry(e)) {
        Some(name) => Some(String::from_utf8_lossy(&extract_to_memory(path, name)?).into_owned()),
        None => None,
    };

    Ok((pages, xml))
}

pub fn read_cover(path: &Path) -> Result<Option<Vec<u8>>> {
    let entries = list_entries(path)?;
    match cover_entry(entries.iter().map(String::as_str)) {
        Some(name) => Ok(Some(extract_to_memory(path, name)?)),
        None => Ok(None),
    }
}

/// Extracts the whole archive into `dest`.
pub fn extract_all(path: &Path, dest: &Path) -> Result<()> {
    // unrar treats a trailing separator as "extract into this directory".
    let mut dest = dest.as_os_str().to_owned();
    dest.push(std::path::MAIN_SEPARATOR_STR);
    run_unrar(&["x", "-o+", "-inul"], path, &[&dest])?;
    Ok(())
}

fn list_entries(path: &Path) -> Result<Vec<String>> {
    let stdout = run_unrar(&["lb"], path, &[])?;
    Ok(String::from_utf8_lossy(&stdout)
        .lines()
        .map(str::to_owned)
        .collect())
}

fn extract_to_memory(path: &Path, entry: &str) -> Result<Vec<u8>> {
    run_unrar(&["p", "-inul"], path, &[OsStr::new(entry)])
}

/// Runs `unrar <args> -p- -- <archive> <trailing...>` and returns its stdout.
fn run_unrar(args: &[&str], archive: &Path, trailing: &[&OsStr]) -> Result<Vec<u8>> {
    let mut cmd = tool("unrar");
    // -p- : never prompt for a password.
    cmd.args(args).arg("-p-").arg("--").arg(archive).args(trailing);

    let output = cmd.output().context("failed to run `unrar` (is it installed?)")?;
    if !output.status.success() {
        bail!(
            "unrar failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}
