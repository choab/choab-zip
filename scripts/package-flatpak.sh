#!/usr/bin/env bash
#
# Build a Flatpak for choab-zip and bundle it into dist/*.flatpak
#
#   ./scripts/package-flatpak.sh
#
# Requirements:
#   * flatpak
#   * org.freedesktop.Sdk//25.08 and org.freedesktop.Platform//25.08 installed
#   * optionally flatpak-builder, on the system or as the flatpak app:
#       flatpak install --user -y flathub org.flatpak.Builder
#
# Without flatpak-builder (or without network access to flathub) the script
# falls back to `flatpak build-init` / `build-finish` / `build-export` — the
# same builder-free path choab-player uses. In that mode the binary is compiled
# on the host with `cargo build --release`, so no Rust toolchain is needed
# inside the sandbox and nothing is downloaded.
#
# Everything is installed in the *user* installation, so no root is needed.
# Set INSTALL_SYSTEM=1 to use the system installations instead.
#
# Output:
#   dist/repo/                                     (local OSTree repo)
#   dist/io.github.choab.Zip.flatpak    (single-file bundle)
#
# Install the bundle with:
#   flatpak install --user dist/io.github.choab.Zip.flatpak
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

APP_ID=io.github.choab.Zip
RUNTIME_VERSION=25.08
DIST="$ROOT/dist"
BUILD_DIR="$DIST/flatpak-build"
REPO="$DIST/repo"
BUNDLE="$DIST/$APP_ID.flatpak"

# --- pick a builder ---------------------------------------------------------
BUILDER=()
if command -v flatpak-builder >/dev/null 2>&1; then
    BUILDER=(flatpak-builder)
elif flatpak info --user org.flatpak.Builder >/dev/null 2>&1; then
    BUILDER=(flatpak run --command=flatpak-builder org.flatpak.Builder)
elif flatpak info --system org.flatpak.Builder >/dev/null 2>&1; then
    BUILDER=(flatpak run --command=flatpak-builder org.flatpak.Builder)
fi

INSTALL_FLAG=(--user)
if [ "${INSTALL_SYSTEM:-0}" = "1" ]; then
    INSTALL_FLAG=()
fi

SDK_REF="org.freedesktop.Sdk/x86_64/$RUNTIME_VERSION"
EXT_REF="org.freedesktop.Sdk.Extension.rust-stable/x86_64/$RUNTIME_VERSION"

mkdir -p "$DIST"

if [ ${#BUILDER[@]} -gt 0 ]; then
    # flatpak-builder (even when run from the Builder flatpak) can only see the
    # *system* installation for runtimes/extensions. If the Rust SDK extension
    # is not installed there, fall back to the manifest that brings its own
    # toolchain.
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

    echo "==> ${BUILDER[*]}"
    "${BUILDER[@]}" \
        "${INSTALL_FLAG[@]}" \
        --force-clean \
        --disable-rofiles-fuse \
        --repo="$REPO" \
        --state-dir="$DIST/.flatpak-builder" \
        "$BUILD_DIR" \
        "$MANIFEST"
else
    # --- builder-free path -------------------------------------------------
    # Same layout, permissions and icons as packaging/flatpak/$APP_ID.yml;
    # keep the two in sync when either changes.
    echo "note: flatpak-builder not found; using the builder-free path"

    # The binary is compiled with the *host* toolchain, so it links against the
    # host glibc and needs a runtime at least that new. Pick the newest
    # installed Platform branch (the manifest's 25.08 would only be right if the
    # build ran inside that SDK).
    if [ -z "${BUNDLE_RUNTIME_BRANCH:-}" ]; then
        BUNDLE_RUNTIME_BRANCH="$(flatpak list --runtime --columns=ref 2>/dev/null \
            | sed -n 's|^org\.freedesktop\.Platform/x86_64/\(.*\)$|\1|p' | sort -V | tail -1)"
    fi
    if [ -z "$BUNDLE_RUNTIME_BRANCH" ]; then
        echo "error: no org.freedesktop.Platform runtime installed." >&2
        exit 1
    fi

    RT_REF="org.freedesktop.Platform/x86_64/$BUNDLE_RUNTIME_BRANCH"
    if ! flatpak list --runtime --columns=ref 2>/dev/null | grep -qx "$RT_REF"; then
        echo "error: $RT_REF is not installed." >&2
        echo "       install it first:  flatpak install -y flathub $RT_REF" >&2
        exit 1
    fi

    # Warn when the host glibc is newer than the runtime's: the resulting
    # binary would fail at startup with "version GLIBC_2.xx not found".
    HOST_GLIBC="$(ldd --version 2>/dev/null | sed -n '1s/.* //p')"
    RT_LOC="$(flatpak info --show-location "$RT_REF" 2>/dev/null || true)"
    RT_GLIBC=""
    if [ -n "$RT_LOC" ]; then
        rt_libc="$(find "$RT_LOC" -maxdepth 4 -name libc.so.6 -print -quit 2>/dev/null)"
        [ -n "$rt_libc" ] && RT_GLIBC="$(strings "$rt_libc" | grep -o 'GLIBC_2\.[0-9]*' \
            | sort -uV | tail -1 | sed 's/^GLIBC_//')"
    fi
    if [ -n "$HOST_GLIBC" ] && [ -n "$RT_GLIBC" ] && [ "$RT_GLIBC" != "$HOST_GLIBC" ] \
        && [ "$(printf '%s\n%s\n' "$RT_GLIBC" "$HOST_GLIBC" | sort -V | tail -1)" = "$HOST_GLIBC" ]; then
        echo "error: runtime $BUNDLE_RUNTIME_BRANCH ships glibc $RT_GLIBC but this host" >&2
        echo "       builds against glibc $HOST_GLIBC, so the binary would not start." >&2
        echo "       Use a newer branch:  BUNDLE_RUNTIME_BRANCH=<branch> $0" >&2
        exit 1
    fi
    echo "==> runtime branch $BUNDLE_RUNTIME_BRANCH (glibc ${RT_GLIBC:-?}, host glibc ${HOST_GLIBC:-?})"

    echo "==> cargo build --release --locked (host toolchain)"
    cargo build --release --locked

    rm -rf "$BUILD_DIR"
    echo "==> writing Flatpak metadata (no SDK needed)"
    mkdir -p "$BUILD_DIR/files"
    cat > "$BUILD_DIR/metadata" <<EOF
[Application]
name=$APP_ID
runtime=org.freedesktop.Platform/x86_64/$BUNDLE_RUNTIME_BRANCH
sdk=org.freedesktop.Sdk/x86_64/$BUNDLE_RUNTIME_BRANCH
command=choab-zip
EOF

    echo "==> staging application files"
    install -Dm755 target/release/choab-zip "$BUILD_DIR/files/bin/choab-zip"
    install -Dm644 "packaging/flatpak/$APP_ID.desktop" \
        "$BUILD_DIR/files/share/applications/$APP_ID.desktop"
    for size in 16 22 24 32 48 64 128 256 512; do
        src="assets/icon${size}.png"
        [ "$size" = "256" ] && src="assets/icon.png"
        install -Dm644 "$src" \
            "$BUILD_DIR/files/share/icons/hicolor/${size}x${size}/apps/$APP_ID.png"
    done
    install -Dm644 "packaging/metainfo/$APP_ID.metainfo.xml" \
        "$BUILD_DIR/files/share/metainfo/$APP_ID.metainfo.xml"

    echo "==> flatpak build-finish"
    flatpak build-finish "$BUILD_DIR" \
        --command=choab-zip \
        --share=ipc \
        --socket=wayland \
        --socket=fallback-x11 \
        --device=dri \
        --filesystem=home \
        --filesystem=/media \
        --filesystem=/run/media \
        --talk-name=org.freedesktop.portal.Desktop \
        --talk-name=org.freedesktop.portal.Documents

    echo "==> flatpak build-export"
    flatpak build-export --disable-sandbox --disable-fsync "$REPO" "$BUILD_DIR" stable
fi

echo "==> flatpak build-bundle"
flatpak build-bundle "$REPO" "$BUNDLE" "$APP_ID" stable

echo
echo "built: $BUNDLE"
echo "  $(du -h "$BUNDLE" | cut -f1)"
echo
echo "install with:  flatpak install --user $BUNDLE"
echo "run with:      flatpak run $APP_ID"
echo
echo "note: the bundle does not replace the runtime; the user still needs"
echo "      org.freedesktop.Platform//${BUNDLE_RUNTIME_BRANCH:-$RUNTIME_VERSION} installed."
