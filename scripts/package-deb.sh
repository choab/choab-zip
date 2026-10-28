#!/usr/bin/env bash
#
# Build a Debian/Ubuntu .deb for 7rust-zip.
#
#   ./scripts/package-deb.sh            # release build + package
#   SKIP_BUILD=1 ./scripts/package-deb.sh
#
# Output: dist/7rust-zip_<version>_<arch>.deb
#
# The binary is linked against the build machine's glibc, so the package's
# libc6 requirement is derived from the symbols the binary actually needs.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PKG=7rust-zip
BIN=target/release/7rust-zip
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)"
ARCH_DEB="$(dpkg --print-architecture)"
CARGO_ARCH="$(rustc -vV | sed -n 's/^host: //p')"
DIST="$ROOT/dist"
mkdir -p "$DIST"
# Stage outside the source tree: the checkout may live on a filesystem that
# cannot represent Unix permissions (NTFS/exFAT/9p), and dpkg-deb rejects a
# world-writable DEBIAN directory.
STAGE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/7rust-zip-deb.XXXXXX")"
STAGE="$STAGE_ROOT/${PKG}_${VERSION}_${ARCH_DEB}"
trap 'rm -rf "$STAGE_ROOT"' EXIT

[ -n "$VERSION" ] || { echo "could not read version from Cargo.toml" >&2; exit 1; }

if [ "${SKIP_BUILD:-0}" != "1" ]; then
    echo "==> cargo build --release"
    cargo build --release --locked
fi
[ -x "$BIN" ] || { echo "missing $BIN" >&2; exit 1; }

echo "==> staging ${STAGE}"
install -Dm755 "$BIN"                      "$STAGE/usr/bin/$PKG"
install -Dm644 assets/7rust-zip.desktop    "$STAGE/usr/share/applications/$PKG.desktop"
install -Dm644 assets/icon.png             "$STAGE/usr/share/icons/hicolor/256x256/apps/$PKG.png"
install -Dm644 assets/icon512.png          "$STAGE/usr/share/icons/hicolor/512x512/apps/$PKG.png"
install -Dm644 packaging/metainfo/io.github.rustzip.SevenRustZip.metainfo.xml \
                                           "$STAGE/usr/share/metainfo/$PKG.metainfo.xml"
install -Dm644 README.md                   "$STAGE/usr/share/doc/$PKG/README.md"
install -Dm644 packaging/deb/copyright     "$STAGE/usr/share/doc/$PKG/copyright"
install -Dm755 packaging/deb/postinst      "$STAGE/DEBIAN/postinst"
install -Dm755 packaging/deb/postrm        "$STAGE/DEBIAN/postrm"

# Compressed changelog (policy-friendly, keeps lintian quiet-ish)
mkdir -p "$STAGE/usr/share/doc/$PKG"
{
    echo "$PKG ($VERSION) unstable; urgency=medium"
    echo
    echo "  * Release $VERSION."
    echo
    echo " -- ${MAINTAINER:-$(git config user.name 2>/dev/null || echo '7rust-zip maintainers')} <${MAINTAINER_EMAIL:-$(git config user.email 2>/dev/null || echo 'maintainers@localhost')}>  $(date -R)"
} | gzip -9n > "$STAGE/usr/share/doc/$PKG/changelog.gz"

# --- dependency analysis ----------------------------------------------------
# Only libc/libm/libgcc/libstdc++ are DT_NEEDED (X11/Wayland/GTK are dlopen'ed),
# so derive the list from ldd and map to packages, with symbol-level libc6.
mapfile -t SONAMES < <(ldd "$BIN" | awk '/=>/ {print $1}')
DEPENDS=()
for so in "${SONAMES[@]}"; do
    case "$so" in
        libc.so.6)      DEPENDS+=("libc6 (>= 2.39)") ;;
        libm.so.6)      : ;;                 # part of libc6
        libgcc_s.so.1)  DEPENDS+=("libgcc-s1") ;;
        libstdc++.so.6) DEPENDS+=("libstdc++6 (>= 12)") ;;
        *)              echo "note: unmapped shared object $so" >&2 ;;
    esac
done
DEP_LIST="$(IFS=,; echo "${DEPENDS[*]}" | sed 's/,/, /g')"

SIZE_KB="$(du -sk --exclude=DEBIAN "$STAGE" | cut -f1)"
MAINTAINER="${MAINTAINER:-$(git config user.name 2>/dev/null || echo '7rust-zip maintainers')}"
MAINTAINER_EMAIL="${MAINTAINER_EMAIL:-$(git config user.email 2>/dev/null || echo 'maintainers@localhost')}"

sed -e "s|@VERSION@|$VERSION|g" \
    -e "s|@ARCH@|$ARCH_DEB|g" \
    -e "s|@SIZE@|$SIZE_KB|g" \
    -e "s|@DEPENDS@|$DEP_LIST|g" \
    -e "s|@MAINTAINER@|$MAINTAINER <$MAINTAINER_EMAIL>|g" \
    packaging/deb/control.in > "$STAGE/DEBIAN/control"

chmod 0644 "$STAGE/DEBIAN/control"
chmod 0755 "$STAGE/DEBIAN/postinst" "$STAGE/DEBIAN/postrm"
find "$STAGE" -type d -exec chmod 0755 {} +

DEB="$DIST/${PKG}_${VERSION}_${ARCH_DEB}.deb"
echo "==> dpkg-deb --build"
dpkg-deb --root-owner-group -Zxz --build "$STAGE" "$DEB" >/dev/null

echo
echo "built: ${DEB}"
echo "  $(du -h "$DEB" | cut -f1)   $(dpkg-deb -f "$DEB" Depends)"
echo
echo "install with:  sudo apt install $DEB"
echo "or:            sudo dpkg -i $DEB && sudo apt-get -f install"
echo "inspect with:  dpkg-deb -I $DEB && dpkg-deb -c $DEB"

