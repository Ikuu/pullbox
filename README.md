# Pullbox

A desktop app for managing, tagging and compressing CBZ/CBR comic libraries.

## Features

- **Library browser**: add one or more library folders and browse them in a
  folder tree, as a sortable list or as a grid of covers. Covers show the
  issue number and badges for CBR, uncompressed and untagged comics. Search by
  series, title, publisher, writer, story arc or file name. **Show in Library**
  goes from Recently Added or a search to the comic in its folder.
- **Comic and manga folders**: when you add a folder, say whether it holds
  comics or manga (it guesses from the folder's name; right-click it to change
  this later). Comics in a manga folder are tagged as manga, read right to
  left.
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
- **Details pane**: the selected comic's cover, credits and summary, or a
  summary of several selected comics. Toggle it with the Details button.
- **Fits your desktop**: uses the KDE colour scheme, font and icon theme, and
  restyles itself when you change them. Elsewhere it uses Breeze Dark colours
  and the system font. Right-click the list's header to choose its columns.
- **Metadata editing**: edit the `ComicInfo.xml` inside a comic, or change
  several comics at once.
- **ComicVine lookup**: match comics against [ComicVine](https://comicvine.gamespot.com/)
  and fill in their metadata. Narrow the series found by year or publisher,
  and ignore publishers you never want, such as those of foreign-language
  editions.
- **AniList lookup**: match manga against [AniList](https://anilist.co/) and
  fill in the series' metadata: titles, story and art credits, genres, tags,
  summary and rating. AniList doesn't list volumes or chapters, so those come
  from the comic's tags or its file name (`v01`, `Vol. 3`, `c097`,
  `Chapter 12`, …) and can be corrected before applying. No API key is needed.
- **Story arcs**: right-click a folder in a manga library and choose
  **Story Arcs…** to split a series into named arcs. Select a run of files and
  make them an arc, or type the volumes and chapters each arc spans
  (`3-14`, `1-6, 8`), so the same arcs fit volumes, chapters or a mix. Applying
  writes each file's arc and its part in it (`StoryArc`, `StoryArcNumber`).
  Arcs can be exported to a JSON preset and imported for another copy of the
  series:

  ```json
  {
    "series": "Berserk",
    "arcs": [
      { "name": "Black Swordsman", "volumes": "1-3", "chapters": "" },
      { "name": "Golden Age", "volumes": "4-13", "chapters": "" }
    ]
  }
  ```
- **Convert to CBZ**: repack CBR (RAR) comics as CBZ.
- **AVIF compression**: re-encode pages as AVIF to save space, lossy or
  lossless. A page is only replaced if the AVIF version is smaller.

When a comic is converted or compressed, its original file is moved to the
system trash, not deleted.

## Requirements

The AppImage includes everything it needs. Otherwise you need:

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

### AppImage

Download `pullbox-<version>-x86_64.AppImage` from the
[Releases](../../releases) page, make it executable and run it:

```sh
chmod +x pullbox-*-x86_64.AppImage
./pullbox-*-x86_64.AppImage
```

It includes `unrar`, `avifenc` and the libraries they need, so there's nothing
else to install. Like the release binary, it needs glibc 2.39 or newer. To add
it to your application launcher, use a tool such as
[Gear Lever](https://flathub.org/apps/it.mijorus.gearlever) or
[AppImageLauncher](https://github.com/TheAssassin/AppImageLauncher).

### From a release

Download `pullbox-<version>-linux-x86_64.tar.gz` from the
[Releases](../../releases) page, then:

```sh
tar xzf pullbox-*-linux-x86_64.tar.gz
cd pullbox-*-linux-x86_64
./install.sh
```

The binary runs on x86-64 Linux with glibc 2.39 or newer (Ubuntu 24.04,
Debian 13, Fedora 40, Arch and later).

### From source

```sh
./install.sh
```

This builds a release binary.

Either way, `install.sh` installs Pullbox for the current user, along
with a launcher entry and icon, so **Pullbox** shows up in your
application launcher:

| What     | Where                                                    |
| -------- | -------------------------------------------------------- |
| Binary   | `~/.local/bin/pullbox`                                   |
| Launcher | `~/.local/share/applications/pullbox.desktop`            |
| Icon     | `~/.local/share/icons/hicolor/scalable/apps/pullbox.svg` |

Set `PREFIX` to install somewhere other than `~/.local`. To remove the app,
run `./install.sh --uninstall`. This leaves your library database, settings
and thumbnails in place.

To run it without installing:

```sh
cargo run --release
```

## Getting started

1. Choose **File → Add Library Folder…**, pick the folder holding your
   comics and say whether it holds comics or manga. The library is scanned in
   the background.
2. To use ComicVine lookups, get a free API key from
   <https://comicvine.gamespot.com/api/> and enter it in **File → Settings…**.
3. Select comics and use the toolbar, or right-click them, to edit their
   info, look them up (on ComicVine, or AniList for manga), convert them or
   compress them.

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
| `Ctrl+F`          | Search                                                    |
| `Escape`          | Clear the search                                          |

Clicking a sidebar entry that's already selected also refreshes it.

## Where data is stored

| What                       | Where (Linux)                         |
| -------------------------- | ------------------------------------- |
| Library database           | `~/.local/share/pullbox/library.db`   |
| Window layout              | `~/.local/share/pullbox/window.ron`   |
| Settings (incl. API key)   | `~/.config/pullbox/settings.json`     |
| Cover thumbnails           | `~/.cache/pullbox/thumbnails/`        |

Set `PULLBOX_DB` to use a different library database.

Pullbox used to be called Comic Editor. If you used it under that name, the
first launch moves your library, settings and thumbnails from the
`comic-editor` folders, and `install.sh` removes the old launcher entry.

## Development

Building needs the dav1d development files (`libdav1d-dev` on Debian/Ubuntu;
included in Arch's `dav1d` package).

```sh
cargo test                # unit tests, no other tools needed
cargo test -- --ignored   # the tests that need avifenc or contact ComicVine or AniList
cargo clippy --all-targets
```

GitHub Actions runs clippy and the tests on every push to `main` and on pull
requests, including the ignored ones except the one that contacts ComicVine.

### Releasing

1. Bump `version` in `Cargo.toml`.
2. Add a section for the version to `CHANGELOG.md`, listing everything that
   changed since the previous release under Additions, Changes and Bug Fixes,
   and commit.
3. Tag the commit with that version and push the tag:

   ```sh
   git tag v0.4.1
   git push origin v0.4.1
   ```

The release workflow builds the binary on Ubuntu 24.04, packages
`pullbox-<tag>-linux-x86_64.tar.gz` and `pullbox-<tag>-x86_64.AppImage`
with their `.sha256` files, and publishes them as a GitHub release. The release
notes are the version's section of `CHANGELOG.md`, followed by install
instructions (`packaging/release-install.md`) and a link to the commits since
the previous release. It fails if the tag doesn't match `Cargo.toml` or
`CHANGELOG.md` has no section for the version. To try a build without
releasing, run the workflow by hand (**Actions → Release → Run workflow**); the
files are attached to the run.

The AppImage is built by `packaging/appimage.sh` with
[linuxdeploy](https://github.com/linuxdeploy/linuxdeploy), and bundles the
build machine's `unrar` and `avifenc`. Both packages put the licence notices
for everything built into the app in `THIRD_PARTY_LICENSES.txt`, generated by
`packaging/third-party-licenses.sh` with
[cargo-about](https://github.com/EmbarkStudios/cargo-about). It lists the
licences dependencies may use in `packaging/about.toml`; a new dependency
under any other licence fails the release until it's been added there.

## Licence

Pullbox is released under the [MIT licence](LICENSE). **Help → About
Pullbox** lists the main projects it's built on, and opens the licences of
everything it includes.
