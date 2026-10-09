# Comic Editor

A desktop app for managing, tagging and compressing CBZ/CBR comic libraries.

## Features

- **Library browser**: add one or more library folders and browse them in a
  folder tree, as a sortable list or as a grid of covers. Search by series,
  title, publisher, writer or file name.
- **Recently Added**: the newest comics in the library, newest first. How far
  back it looks is set in Settings (30 days by default).
- **Metadata editing**: edit the `ComicInfo.xml` inside a comic, or change
  several comics at once.
- **ComicVine lookup**: match comics against [ComicVine](https://comicvine.gamespot.com/)
  and fill in their metadata.
- **Convert to CBZ**: repack CBR (RAR) comics as CBZ.
- **AVIF compression**: re-encode pages as AVIF to save space, lossy or
  lossless. A page is only replaced if the AVIF version is smaller.

When a comic is converted or compressed, its original file is moved to the
system trash, not deleted.

## Requirements

- [Rust](https://rustup.rs/) 1.95 or newer, to build.
- `unrar`, to read CBR files.
- `avifenc` from libavif, for AVIF compression.

Installing the two tools:

| Distribution  | Command                                |
| ------------- | -------------------------------------- |
| Arch          | `sudo pacman -S unrar libavif`         |
| Debian/Ubuntu | `sudo apt install unrar libavif-bin`   |

## Installing

```sh
./install.sh
```

This builds a release binary and installs it for the current user, along with
a launcher entry and icon, so **Comic Editor** shows up in your application
launcher:

| What     | Where                                                           |
| -------- | --------------------------------------------------------------- |
| Binary   | `~/.local/bin/comic-editor`                                     |
| Launcher | `~/.local/share/applications/comic-editor.desktop`              |
| Icon     | `~/.local/share/icons/hicolor/scalable/apps/comic-editor.svg`   |

Set `PREFIX` to install somewhere other than `~/.local`. To remove the app,
run `./install.sh --uninstall`. This leaves your library database, settings
and thumbnails in place.

To run it without installing:

```sh
cargo run --release
```

## Getting started

1. Choose **File → Add Library Folder…** and pick the folder holding your
   comics. The library is scanned in the background.
2. To use ComicVine lookups, get a free API key from
   <https://comicvine.gamespot.com/api/> and enter it in **File → Settings…**.
3. Right-click comics to edit their info, look them up on ComicVine, convert
   them or compress them.

The library is rescanned on every launch. Only new or changed files are read
again, so this is quick.

## Shortcuts

| Key               | Action                                                    |
| ----------------- | --------------------------------------------------------- |
| `Ctrl+A`          | Select all comics in view                                 |
| `Ctrl+I`          | Edit info for the selected comics                         |
| Double-click      | Edit info for that comic                                  |
| `F5`              | Refresh the selected folder, or the whole library         |

Clicking a sidebar entry that's already selected also refreshes it.

## Where data is stored

| What                       | Where (Linux)                              |
| -------------------------- | ------------------------------------------ |
| Library database           | `~/.local/share/comic-editor/library.db`   |
| Window layout              | `~/.local/share/comic-editor/window.ron`   |
| Settings (incl. API key)   | `~/.config/comic-editor/settings.json`     |
| Cover thumbnails           | `~/.cache/comic-editor/thumbnails/`        |

Set `COMIC_EDITOR_DB` to use a different library database.

## Development

```sh
cargo test                # unit tests
cargo test -- --ignored   # also the tests that need avifenc or contact ComicVine
cargo clippy --all-targets
```
