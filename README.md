# Comic Editor

A desktop app for managing, tagging and compressing CBZ/CBR comic libraries.

## Features

- **Library browser**: add one or more library folders and browse them in a
  folder tree, as a sortable list or as a grid of covers. Search by series,
  title, publisher, writer or file name.
- **Recently Added**: the newest comics in the library, newest first. How far
  back it looks is set in Settings (30 days by default).
- **Series check**: finds missing issues, issues you have more than once and
  identical files, for the whole library (File menu) or one folder (right-click
  it). Trades count for the issues they collect, read from "Collects …" in their
  summary or entered by you.
- **Library stats**: total size, space saved by compression, untagged
  comics, and size by format, series and publisher, for the whole library
  (File menu) or one folder (right-click it). Savings are only known for comics
  compressed since the stats were added.
- **Details pane**: the selected comic's cover, credits and summary. Toggle it
  with the Details button.
- **Fits your desktop**: uses the KDE colour scheme, font and icon theme, and
  restyles itself when you change them. Elsewhere it uses Breeze Dark colours
  and the system font. Right-click the list's header to choose its columns.
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

- `dav1d`, to show AVIF covers.
- `unrar`, to read CBR files.
- `avifenc` from libavif, for AVIF compression.
- [Rust](https://rustup.rs/) 1.95 or newer, only to build from source.

Installing them:

| Distribution  | Command                                             |
| ------------- | --------------------------------------------------- |
| Arch          | `sudo pacman -S dav1d unrar libavif`                |
| Debian/Ubuntu | `sudo apt install libdav1d7 unrar libavif-bin`      |

## Installing

### From a release

Download `comic-editor-<version>-linux-x86_64.tar.gz` from the
[Releases](../../releases) page, then:

```sh
tar xzf comic-editor-*-linux-x86_64.tar.gz
cd comic-editor-*-linux-x86_64
./install.sh
```

The binary runs on x86-64 Linux with glibc 2.39 or newer (Ubuntu 24.04,
Debian 13, Fedora 40, Arch and later).

### From source

```sh
./install.sh
```

This builds a release binary.

Either way, `install.sh` installs Comic Editor for the current user, along
with a launcher entry and icon, so **Comic Editor** shows up in your
application launcher:

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
again, so this is quick. Hidden files and folders (names starting with `.`)
are skipped. If a library folder can't be found, for example on a drive that
isn't mounted, its comics stay in the library until it's back.

When a scan or job can't process some comics, the status bar shows a
**⚠ problems** link. Click it to see which files failed and why.

Quitting while a job is running asks first, and then stops after the comic
being worked on, so no comic is left half-finished.

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

Building needs the dav1d development files (`libdav1d-dev` on Debian/Ubuntu;
included in Arch's `dav1d` package).

```sh
cargo test                # unit tests, no other tools needed
cargo test -- --ignored   # the tests that need avifenc or contact ComicVine
cargo clippy --all-targets
```

GitHub Actions runs clippy and the tests (except the ComicVine one) on every
push to `main` and on pull requests.

### Releasing

1. Bump `version` in `Cargo.toml` and commit.
2. Tag the commit with that version and push the tag:

   ```sh
   git tag v0.2.0
   git push origin v0.2.0
   ```

The release workflow builds the binary on Ubuntu 24.04, packages
`comic-editor-<tag>-linux-x86_64.tar.gz` with its `.sha256`, and publishes
them as a GitHub release. It fails if the tag doesn't match `Cargo.toml`.
