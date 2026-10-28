#!/usr/bin/env sh
# Builds target/appimage/pullbox-v<version>-x86_64.AppImage from the
# release binary, with unrar, avifenc and the libraries they and the app need
# bundled, so it runs without installing anything.
#
#   cargo build --release --locked
#   packaging/appimage.sh
#
# Needs unrar and avifenc installed, cargo-about (see
# third-party-licenses.sh), and linuxdeploy on PATH or named by LINUXDEPLOY
# (https://github.com/linuxdeploy/linuxdeploy/releases). Graphics drivers,
# glibc and the display server libraries come from the host system.
set -eu

cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
linuxdeploy=${LINUXDEPLOY:-linuxdeploy}
build=$PWD/target/appimage
appdir=$build/AppDir
unrar=$(command -v unrar)
avifenc=$(command -v avifenc)

rm -rf "$build"
mkdir -p "$build"

# Inside the AppImage the launcher refers to the app by name, not by path.
sed -e '/^#/d' -e 's|@BIN@|pullbox|' -e 's|@ICON@|pullbox|' \
    packaging/pullbox.desktop > "$build/pullbox.desktop"

# The app looks for unrar and avifenc next to its own binary first.
"$linuxdeploy" --appdir "$appdir" \
    --executable target/release/pullbox \
    --executable "$unrar" \
    --executable "$avifenc" \
    --desktop-file "$build/pullbox.desktop" \
    --icon-file packaging/pullbox.svg

# The system copies of the libraries linuxdeploy bundled, to find out which
# packages (and so which licences) they came from.
libraries=$(ldd target/release/pullbox "$unrar" "$avifenc" | awk '/=> \// { print $3 }' | sort -u |
    while read -r lib; do
        if [ -e "$appdir/usr/lib/$(basename "$lib")" ]; then echo "$lib"; fi
    done)
mkdir -p "$appdir/usr/share/doc/pullbox"
cp LICENSE "$appdir/usr/share/doc/pullbox/"
# shellcheck disable=SC2086 # one library per word
packaging/third-party-licenses.sh "$appdir/usr/share/doc/pullbox/THIRD_PARTY_LICENSES.txt" \
    "$unrar" "$avifenc" $libraries

cd "$build"
LDAI_OUTPUT="pullbox-v$version-x86_64.AppImage" "$linuxdeploy" --appdir "$appdir" --output appimage
echo "Built $build/pullbox-v$version-x86_64.AppImage"
