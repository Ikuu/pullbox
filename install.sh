#!/usr/bin/env sh
# Builds Pullbox and installs it for the current user, so it appears in
# the desktop's application launcher. In an unpacked release download, the
# included binary is installed instead of building one.
#
#
#   binary    $PREFIX/bin/pullbox
#   launcher  $PREFIX/share/applications/pullbox.desktop
#   icon      $PREFIX/share/icons/hicolor/scalable/apps/pullbox.svg
#   licences  $PREFIX/share/doc/pullbox/
#
# PREFIX defaults to ~/.local. Run with --uninstall to remove these files
# (your library database, settings and thumbnail cache are left alone).
set -eu

cd "$(dirname "$0")"
PREFIX="${PREFIX:-$HOME/.local}"
BIN="$PREFIX/bin/pullbox"
DESKTOP="$PREFIX/share/applications/pullbox.desktop"
ICON="$PREFIX/share/icons/hicolor/scalable/apps/pullbox.svg"
DOC="$PREFIX/share/doc/pullbox"

refresh_desktop_caches() {
    # Make the launcher notice the change without logging out.
    command -v update-desktop-database >/dev/null && update-desktop-database -q "$PREFIX/share/applications" || true
    command -v kbuildsycoca6 >/dev/null && kbuildsycoca6 --noincremental >/dev/null 2>&1 || true
    # KDE caches icon lookups; drop the cache so a new icon shows up.
    rm -f "${XDG_CACHE_HOME:-$HOME/.cache}/icon-cache.kcache"
}

# Pullbox used to be called Comic Editor; remove an install under that name.
rm -f "$PREFIX/bin/comic-editor" "$PREFIX/share/applications/comic-editor.desktop" \
    "$PREFIX/share/icons/hicolor/scalable/apps/comic-editor.svg"
rm -rf "$PREFIX/share/doc/comic-editor"

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$BIN" "$DESKTOP" "$ICON"
    rm -rf "$DOC"
    refresh_desktop_caches
    echo "Pullbox uninstalled."
    exit 0
fi

if [ -f pullbox ]; then
    # Unpacked from a release download: the binary is already built.
    BUILT=pullbox
else
    echo "Building release binary…"
    cargo build --release --locked
    BUILT=target/release/pullbox
fi

install -Dm755 "$BUILT" "$BIN"
install -Dm644 packaging/pullbox.svg "$ICON"
mkdir -p "$(dirname "$DESKTOP")"
sed -e "s|@BIN@|$BIN|" -e "s|@ICON@|$ICON|" packaging/pullbox.desktop > "$DESKTOP"
chmod 644 "$DESKTOP"
install -Dm644 LICENSE "$DOC/LICENSE"
# Release downloads include the third-party licences, shown in Help > About.
if [ -f THIRD_PARTY_LICENSES.txt ]; then
    install -Dm644 THIRD_PARTY_LICENSES.txt "$DOC/THIRD_PARTY_LICENSES.txt"
fi

refresh_desktop_caches
echo "Installed Pullbox to $BIN"
echo "Search for \"Pullbox\" in your application launcher."
