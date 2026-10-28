# Changelog

What changed in each release. The release workflow publishes a version's
section as its GitHub release notes, followed by
`packaging/release-install.md`. Keep each entry on one line: GitHub shows
line breaks in release notes as they are.

## 0.4.1 – 2026-10-28

### Changes
- Comic Editor is now called Pullbox. The library, settings and thumbnails are moved to their new folders on first launch, and installing removes the old launcher entry. The `COMIC_EDITOR_DB` environment variable is now `PULLBOX_DB`

## 0.4.0 – 2026-10-28

### Additions
- Manga libraries: when you add a library folder, say whether it holds comics or manga (it guesses from the folder's name). Right-click a library folder to change it later. Manga folders get their own icon, and editing an untagged comic in one offers to mark it as right-to-left manga
- AniList lookup for manga: comics in manga folders are looked up on AniList instead of ComicVine, with no API key needed. It fills in the series and its other title, story and art credits, genres, tags, summary, rating, reading direction and volume or chapter count. Volume and chapter numbers come from the file's tags or name and can be corrected before applying
- Story arcs (right-click a folder in a manga library → Story Arcs…): split each series into named arcs by selecting a run of files or typing the volumes and chapters each arc spans ("3-14", "1-6, 8"). Writes StoryArc and StoryArcNumber, and arcs can be exported to and imported from a preset file
- The search box also finds comics by story arc
- ComicVine lookup: filter the series found by start year or publisher
- ComicVine lookup: ignore publishers, such as those of foreign-language editions, from a result's right-click menu or the Ignored Publishers menu. Their series are hidden from the results and skipped when matching automatically
- Show in Library in a comic's right-click menu, to go from Recently Added, All Comics or a search to the comic in its folder

### Changes
- ComicVine series results are listed newest first, in a taller list
- Showing a comic in its folder, from Show in Library or Check Series, expands the folder tree to that folder and scrolls to it
- The first scan after updating re-reads every comic once, to record its story arc

### Bug Fixes
- Fixed ComicVine and AniList lookups showing the previous search's results while a new search runs
- Fixed long titles and credit lists in a lookup's match preview stretching the window past its edge
- Fixed list rows and folders still moving slightly when hovered, nudging their text and the rows below
- Fixed labels in the details pane wrapping into a narrow column

## 0.3.0 – 2026-10-24

### Additions
- An AppImage download that includes unrar, avifenc and everything else it needs, so nothing has to be installed first
- Series check (File → Check Series, or right-click a folder): lists missing issues, issues you have more than once and identical files. Trades count for the issues they collect, read from "Collects …" in their summary or entered by you
- Library stats (File → Library Stats, or right-click a folder): total size, space saved by compression, how much is tagged, and size by format, series and publisher
- Details pane showing the selected comic's cover, facts, credits and summary, or a summary of a multiple selection. Toggle it with the Details button
- Help → About Comic Editor: version, licence, the external tools in use and the projects the app is built on, with their full licences
- Toolbar buttons for Edit Info, Look Up, Convert and Compress
- Right-click the list header to choose which columns are shown
- Issue number and CBR, Uncompressed and Untagged badges on grid covers
- Notifications when jobs finish or something goes wrong
- Ctrl+F focuses the search box; Escape clears it

### Changes
- Follows the KDE colour scheme, font and icon theme, and restyles itself when they change. Elsewhere it uses Breeze Dark colours and the system font
- Include subfolders moved to the folder sidebar
- Taller list rows and a clearer list header; dimmer comic counts in the folder tree
- The progress bar's label stays readable over the bar, and a spinner shows while a scan is still counting comics
- Comic Editor is now MIT licensed, and release downloads include the licences of everything built into it

### Bug Fixes
- Fixed list rows, folders and tabs shifting by a couple of pixels when hovered
- Fixed the Library Stats and Check Series windows having an oversized title bar
