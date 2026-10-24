# Changelog

What changed in each release. The release workflow publishes a version's
section as its GitHub release notes, followed by
`packaging/release-install.md`. Keep each entry on one line: GitHub shows
line breaks in release notes as they are.

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
