#!/usr/bin/env sh
# Builds Comic Editor and installs it for the current user, so it appears in
# the desktop's application launcher. In an unpacked release download, the
# included binary is installed instead of building one.
#
#
#   binary    $PREFIX/bin/comic-editor
#   launcher  $PREFIX/share/applications/comic-editor.desktop
#   icon      $PREFIX/share/icons/hicolor/scalable/apps/comic-editor.svg
#   licences  $PREFIX/share/doc/comic-editor/
#
# PREFIX defaults to ~/.local. Run with --uninstall to remove these files
# (your library database, settings and thumbnail cache are left alone).
set -eu

cd "$(dirname "$0")"
PREFIX="${PREFIX:-$HOME/.local}"
BIN="$PREFIX/bin/comic-editor"
DESKTOP="$PREFIX/share/applications/comic-editor.desktop"
ICON="$PREFIX/share/icons/hicolor/scalable/apps/comic-editor.svg"
DOC="$PREFIX/share/doc/comic-editor"

refresh_desktop_caches() {
    # Make the launcher notice the change without logging out.
    command -v update-desktop-database >/dev/null && update-desktop-database -q "$PREFIX/share/applications" || true
    command -v kbuildsycoca6 >/dev/null && kbuildsycoca6 --noincremental >/dev/null 2>&1 || true
    # KDE caches icon lookups; drop the cache so a new icon shows up.
    rm -f "${XDG_CACHE_HOME:-$HOME/.cache}/icon-cache.kcache"
}

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$BIN" "$DESKTOP" "$ICON"
    rm -rf "$DOC"
    refresh_desktop_caches
    echo "Comic Editor uninstalled."
    exit 0
fi

if [ -f comic-editor ]; then
    # Unpacked from a release download: the binary is already built.
    BUILT=comic-editor
else
    echo "Building release binary…"
    cargo build --release --locked
    BUILT=target/release/comic-editor
fi

install -Dm755 "$BUILT" "$BIN"
install -Dm644 packaging/comic-editor.svg "$ICON"
mkdir -p "$(dirname "$DESKTOP")"
sed -e "s|@BIN@|$BIN|" -e "s|@ICON@|$ICON|" packaging/comic-editor.desktop > "$DESKTOP"
chmod 644 "$DESKTOP"
install -Dm644 LICENSE "$DOC/LICENSE"
# Release downloads include the third-party licences, shown in Help > About.
if [ -f THIRD_PARTY_LICENSES.txt ]; then
    install -Dm644 THIRD_PARTY_LICENSES.txt "$DOC/THIRD_PARTY_LICENSES.txt"
fi

refresh_desktop_caches
echo "Installed Comic Editor to $BIN"
echo "Search for \"Comic Editor\" in your application launcher."
