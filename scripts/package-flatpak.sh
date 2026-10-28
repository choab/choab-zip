#!/usr/bin/env bash
#
# Build a Flatpak for 7rust-zip and bundle it into dist/*.flatpak
#
#   ./scripts/package-flatpak.sh
#
# Requirements:
#   * flatpak + the flathub remote
#   * org.freedesktop.Sdk//25.08
#   * flatpak-builder, either on the system or as the flatpak app
#       flatpak install --user -y flathub org.flatpak.Builder
#
# Everything is installed in the *user* installation, so no root is needed.
# Set INSTALL_SYSTEM=1 to use the system installations instead.
#
# Output:
#   dist/repo/                                     (local OSTree repo)
#   dist/io.github.rustzip.SevenRustZip.flatpak    (single-file bundle)
#
# Install the bundle with:
#   flatpak install --user dist/io.github.rustzip.SevenRustZip.flatpak
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

APP_ID=io.github.rustzip.SevenRustZip
RUNTIME_VERSION=25.08
DIST="$ROOT/dist"
BUILD_DIR="$DIST/flatpak-build"
REPO="$DIST/repo"
BUNDLE="$DIST/$APP_ID.flatpak"

# --- pick a builder ---------------------------------------------------------
if command -v flatpak-builder >/dev/null 2>&1; then
    BUILDER=(flatpak-builder)
elif flatpak info --user org.flatpak.Builder >/dev/null 2>&1; then
    BUILDER=(flatpak run --command=flatpak-builder org.flatpak.Builder)
elif flatpak info --system org.flatpak.Builder >/dev/null 2>&1; then
    BUILDER=(flatpak run --command=flatpak-builder org.flatpak.Builder)
else
    cat >&2 <<EOF
flatpak-builder is not installed. Install it without root with:

    flatpak install -y flathub org.flatpak.Builder

and the SDK this manifest needs (already present if 'flatpak list --runtime'
shows org.freedesktop.Sdk//$RUNTIME_VERSION):

    flatpak install -y flathub org.freedesktop.Sdk//$RUNTIME_VERSION
EOF
    exit 1
fi

INSTALL_FLAG=(--user)
if [ "${INSTALL_SYSTEM:-0}" = "1" ]; then
    INSTALL_FLAG=()
fi

SDK_REF="org.freedesktop.Sdk/x86_64/$RUNTIME_VERSION"
EXT_REF="org.freedesktop.Sdk.Extension.rust-stable/x86_64/$RUNTIME_VERSION"

# flatpak-builder (even when run from the Builder flatpak) can only see the
# *system* installation for runtimes/extensions. If the Rust SDK extension is
# not installed there, fall back to the manifest that brings its own toolchain.
MANIFEST="packaging/flatpak/$APP_ID.yml"
if ! flatpak list --runtime --system --columns=ref 2>/dev/null | grep -qx "$EXT_REF"; then
    MANIFEST="packaging/flatpak/$APP_ID.local.yml"
    echo "note: $EXT_REF not in the system installation; using $MANIFEST"
fi
if ! flatpak list --runtime "${INSTALL_FLAG[@]}" --columns=ref 2>/dev/null | grep -qx "$SDK_REF"; then
    echo "==> installing org.freedesktop.Sdk//$RUNTIME_VERSION (large download)"
    flatpak install "${INSTALL_FLAG[@]}" -y flathub \
        "org.freedesktop.Sdk//$RUNTIME_VERSION" \
        "org.freedesktop.Platform//$RUNTIME_VERSION"
fi
if [ "$MANIFEST" = "packaging/flatpak/$APP_ID.yml" ] \
    && ! flatpak list --runtime "${INSTALL_FLAG[@]}" --columns=ref 2>/dev/null | grep -qx "$EXT_REF"; then
    echo "==> installing org.freedesktop.Sdk.Extension.rust-stable//$RUNTIME_VERSION"
    flatpak install "${INSTALL_FLAG[@]}" -y flathub \
        "org.freedesktop.Sdk.Extension.rust-stable//$RUNTIME_VERSION"
fi

mkdir -p "$DIST"

echo "==> ${BUILDER[*]}"
"${BUILDER[@]}" \
    "${INSTALL_FLAG[@]}" \
    --force-clean \
    --disable-rofiles-fuse \
    --repo="$REPO" \
    --state-dir="$DIST/.flatpak-builder" \
    "$BUILD_DIR" \
    "$MANIFEST"

echo "==> flatpak build-bundle"
flatpak build-bundle "$REPO" "$BUNDLE" "$APP_ID" --runtime-repo=https://flathub.org/repo/flathub.flatpakrepo

echo
echo "built: $BUNDLE"
echo "  $(du -h "$BUNDLE" | cut -f1)"
echo
echo "install with:  flatpak install --user $BUNDLE"
echo "run with:      flatpak run $APP_ID"
echo
echo "note: the bundle does not replace the runtime; the user still needs"
echo "      org.freedesktop.Platform//$RUNTIME_VERSION (fetched from flathub)."
