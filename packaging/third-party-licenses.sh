#!/usr/bin/env sh
# Writes the licence notices for everything Pullbox is built from to OUT:
# the Rust crates compiled into the binary, then the system packages that the
# given FILEs (programs and libraries bundled alongside it) came from.
#
#   packaging/third-party-licenses.sh OUT [FILE...]
#
# Needs cargo-about (cargo install cargo-about --locked --features cli).
# Packages are looked up with dpkg (Debian/Ubuntu, as in CI) or pacman (Arch).
set -eu

out=$1
shift
here=$(dirname "$0")
rust=$(mktemp)
trap 'rm -f "$rust"' EXIT

cargo about generate --manifest-path "$here/../Cargo.toml" -c "$here/about.toml" "$here/about.hbs" -o "$rust"
# Long "Used by" lines are easier to read wrapped.
fold -s -w 100 "$rust" > "$out"

[ $# -gt 0 ] || exit 0

# The package each file came from, once per package.
owner() {
    file=$(readlink -f "$1")
    if command -v dpkg-query >/dev/null; then
        dpkg-query -S "$file" | head -n1 | cut -d: -f1
    else
        pacman -Qoq "$file"
    fi
}
packages=$(for file; do owner "$file"; done | sort -u)

cat >> "$out" <<'EOF'


##############################################################################

Pullbox also includes the programs and libraries below, from the Linux
distribution it was built on. Their copyright and licence information follows.
EOF

for package in $packages; do
    if command -v dpkg-query >/dev/null; then
        version=$(dpkg-query -W -f='${Version}' "$package")
        set -- "/usr/share/doc/$package/copyright"
    else
        version=$(pacman -Q "$package" | cut -d' ' -f2)
        set -- /usr/share/licenses/"$package"/*
    fi
    printf '\n==============================================================================\n' >> "$out"
    printf '%s %s\n' "$package" "$version" >> "$out"
    printf '==============================================================================\n\n' >> "$out"
    if [ -f "$1" ]; then
        cat "$@" >> "$out"
    elif command -v pacman >/dev/null; then
        # Arch keeps common licences in /usr/share/licenses/spdx instead.
        pacman -Qi "$package" | grep '^Licenses' >> "$out"
    else
        echo "No licence information found for $package" >&2
        exit 1
    fi
done
