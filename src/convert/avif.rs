//! AVIF re-encoding of extracted comic pages via the external `avifenc` tool.
//! Ported from cbz-compress-avif-rs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result};
use rayon::prelude::*;
use walkdir::WalkDir;

use crate::archive;
use crate::util::tool;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompressOptions {
    /// Lossy quality 0–100; ignored when `lossless`.
    pub quality: u8,
    pub lossless: bool,
}

impl Default for CompressOptions {
    fn default() -> Self {
        Self { quality: 80, lossless: false }
    }
}

/// Fails with installation instructions if `avifenc` isn't available.
pub fn check_avifenc() -> Result<()> {
    tool("avifenc")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("avifenc not found — install libavif (Arch: pacman -S libavif, Debian/Ubuntu: apt install libavif-bin)")?;
    Ok(())
}

/// Re-encodes every image under `dir` to AVIF in place, in parallel.
///
/// Each `avifenc` runs single-threaded since images are encoded concurrently.
/// An image is only replaced when the AVIF version is smaller; failed or
/// larger encodes keep the original. Returns the number of images replaced.
pub fn encode_directory(dir: &Path, options: CompressOptions) -> Result<usize> {
    let images: Vec<PathBuf> = WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && archive::is_recompressible(e.path()))
        .map(walkdir::DirEntry::into_path)
        .collect();

    let replaced: Vec<bool> = images
        .par_iter()
        .map(|src| encode_replacing(src, options))
        .collect::<Result<_>>()?;

    Ok(replaced.iter().filter(|&&r| r).count())
}

/// Encodes `src` to a sibling `.avif`. Returns whether `src` was replaced.
fn encode_replacing(src: &Path, options: CompressOptions) -> Result<bool> {
    let dst = src.with_extension("avif");
    // e.g. both `001.jpg` and `001.png` exist; don't overwrite the other page.
    if dst.exists() {
        return Ok(false);
    }

    if !run_avifenc(src, &dst, options) {
        let _ = fs::remove_file(&dst);
        return Ok(false);
    }

    let src_size = fs::metadata(src)?.len();
    let dst_size = fs::metadata(&dst)?.len();
    if dst_size >= src_size {
        fs::remove_file(&dst)?;
        return Ok(false);
    }
    fs::remove_file(src)?;
    Ok(true)
}

fn run_avifenc(src: &Path, dst: &Path, options: CompressOptions) -> bool {
    let mut cmd = tool("avifenc");
    cmd.args(["-j", "1"]);
    if options.lossless {
        cmd.arg("--lossless");
    } else {
        cmd.args(["-q", &options.quality.to_string()]);
    }
    cmd.arg(src).arg(dst).stdout(Stdio::null()).stderr(Stdio::null());
    cmd.status().is_ok_and(|s| s.success())
}
