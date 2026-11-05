# iob-zip

A native, 7-Zip-style archiver for Linux, written in pure Rust.

No Wine, no Qt, no `7z` subprocess. One ~11 MB self-contained binary.

Archive browsing and text preview are native. Files the desktop handles
better (PDF, images, audio, video, office documents) are opened with the
system default application.

## Why

7-Zip's engine is excellent; its interface belongs to 2005. Linux deserves a
fast, native archiver with a two-pane layout and a modern shell around it —
black, white and grey, real icons, and no windows-inside-windows. That's all
this is.

## Features

| Feature | Status |
|---|---|
| **Archive Formats** (7z, zip, tar, tar.gz, tar.xz, tar.bz2, tar.zst) | list, extract, create |
| **Single-Stream Formats** (gz, xz, bz2, zst) | virtual listing & extract |
| **AES-256 Encryption & Password Protection** | interactive password prompt modal, encrypted 7z creation, header encryption (hidden file names) |
| **Compression Control** | levels 0-9 (Store..Ultra), solid blocks, codecs (LZMA2, LZMA, ZSTD, BZIP2, Deflate, Brotli, LZ4, Copy) |
| **Checksum / Hash Calculator** | pure Rust streaming CRC-32 & SHA-256 calculation dialog with one-click copy |
| **Native Image Viewer** | in-app view for PNG, JPG/JPEG, BMP, GIF, WEBP with zoom and dimensions |
| **Enhanced Text Viewer** | in-app view for code/text with substring search, match counter, word wrap toggle, copy to clipboard |
| **System Default App Delegation** | seamless temporary extraction & external launch for PDFs, audio, video, office docs |
| **Interactive Breadcrumb Navigation** | breadcrumb buttons + inline editable path bar for both filesystem and archive |
| **Multi-Selection & Batch Operations** | Ctrl+Click, Shift+Click, Ctrl+A, batch extract selected, batch delete |
| **Drag & Drop** | drop archives to open, drop files/folders to create archive or navigate |
| **Modern Interface** | header bar, flat command bar, places sidebar, vector icons, inspector viewer, centred dialogs |
| **Theme Switcher** | strictly greyscale, follows the Plasma light/dark scheme; manual toggle (`Ctrl+D`) |
| **Two-Pane Layout** | left filesystem browser, right archive browser (mirrors 7zFM) |
| **Keyboard Navigation & Shortcuts** | F5 (Copy), F7 (New Folder), Delete, Backspace, Enter, Ctrl+PgUp/PgDn, Ctrl+A, Ctrl+D, Esc, type-to-find |
| **Integrity Testing** | native CRC/decompression verification pass |
| **Cross-Platform & Pure Rust** | no Wine, no Qt, no external `7z` subprocesses, 100% pure Rust |

## Interface

![iob-zip, light theme](docs/screenshot-light.png)
![iob-zip, dark theme](docs/screenshot-dark.png)

One window, four regions:

```
header bar      app mark · open archive · window controls
command bar     Add · Extract · Selected · Test · Hashes │ folder & file ops │ Up · Refresh
sidebar         Places (Home, Desktop, Documents, …) and mounted devices
two panes       filesystem on the left, archive contents on the right
```

- The viewer (text, images) docks **below** the panes, so opening a file never
  squeezes the archive list.
- Icons are drawn as vectors on a single weight, so they stay crisp at any
  scale and never need asset files.
- The palette is strictly greyscale — no hue anywhere, including the accent —
  with light and dark variants chosen from the Plasma colour scheme
  (`kdeglobals`: `ColorScheme`) and a manual toggle on `Ctrl+D`.
- Columns drop from Packed to Modified to Size as a pane narrows, instead of
  overlapping.

### External apps only for non-text files

Archive work stays native: no `7z` subprocess, no external archiver. The
built-in viewer is reserved for small text files and images (up to 8 MB). When you open
a PDF, image, video, audio file, office document, or an unknown binary, the
app asks the desktop to open it (`xdg-open`, with `gio open` / `kde-open` as
fallbacks). Files inside archives are decoded to a temporary copy first,
because the desktop needs a real path. Temporary copies live under
`$TMPDIR/iob-zip/` and are left for the OS to clean up.

## Build

```bash
cargo build --release
./target/release/iob-zip
```

Requires a Rust toolchain and X11/Wayland dev headers (for `eframe`).

## Verify the engine without a display

```bash
./target/release/iob-zip --selftest path/to/archive.7z
```

Lists entries and extracts to a temp dir. Useful for CI and for checking
that a given codec works on your machine.

## Layout

- `src/engine.rs` — format detection, listing, extract, create. All archive
  logic lives here and is UI-agnostic, so it can be reused or tested alone.
- `src/main.rs` — application state: navigation, archive operations, worker
  threads and the dialogs.
- `src/ui.rs` — the interface: header bar, command bar, places sidebar, the two
  panes, the viewer dock and the status bar.
- `src/design.rs` — palette (strictly greyscale), type scale, the vector icon
  set and the small widgets everything is built from.
- `assets/icon*.png` — the app icon at every hicolor size (16–512), generated
  by `scripts/make_app_icon.py`; `assets/icon.png` is the 256 px one embedded
  in the binary.
- `assets/iob-zip.desktop` — launcher entry for the application menu.
- `scripts/make_appstream.py` — composes the AppStream catalogue entry that
  Flatpak expects, from the metainfo and the desktop file.

## Glyph guidance (IMPORTANT)

Every icon in the app is drawn with `egui::Painter` in `src/design.rs`, so
there are no icon fonts or PNG assets to keep in sync. Text labels are a
different matter: egui only bundles `Ubuntu-Light`, `Hack`, `NotoEmoji` and a
**subset** of `emoji-icon-font`. Most emoji are **not** included, so characters
such as 📁 📄 📂 📦 🗀 render as tofu boxes (`□`).

Glyphs verified present and safe to use:

```
★ ☆ ▶ ■ □ ● ✔ ✚ ⏏ ⚙ ☰ ✕ ✖ ⬆ ⬇ ⟳ ▣ · • … » ›
```

Do **not** use: 📁 📄 📂 📤 🗁 🗋 🖹 🔍 ▸ ▾ ▪ ◇ ◆ ↑ → ↩ ⌂ and the
box-drawing set (─ │ ├ └). To use a wider set, load a font with
`egui::Context::set_fonts`.

## Packages (.deb / Flatpak)

Both packages are built from the same release binary and ship the launcher
entry, the icon at every standard size (16–512 px) and AppStream metadata.

### Debian / Ubuntu

```bash
./scripts/package-deb.sh                 # cargo build --release + package
sudo apt install ./dist/iob-zip_*_amd64.deb
```

Requirements are derived from the binary's own shared objects
(`libc6`, `libgcc-s1`, `libstdc++6`); X11/Wayland/GTK/DBus are loaded at
runtime, hence only `Recommends` entries. Remove with `sudo apt remove iob-zip`.

Note that the binary links against the build machine's glibc, so building on an
older distro lowers the `libc6` floor (currently 2.39).

### Flatpak

```bash
./scripts/package-flatpak.sh
flatpak install --user dist/io.github.iobzip.IobZip.flatpak
flatpak run io.github.iobzip.IobZip
```

The script uses `packaging/flatpak/io.github.iobzip.IobZip.yml`, which
builds against `org.freedesktop.Sdk//25.08` with the
`org.freedesktop.Sdk.Extension.rust-stable` extension — the manifest to submit
upstream. `flatpak-builder` can only see the *system* installation for
runtimes/extensions; when the extension is missing there, the script falls back
to `...local.yml`, which downloads a pinned Rust toolchain as a build source so
it works with an unprivileged user installation. Install `flatpak-builder`
itself with `flatpak install --user -y flathub org.flatpak.Builder`.

Both scripts stage outside the source tree, so the checkout may live on a
filesystem that cannot store Unix permissions (NTFS/exFAT).

## Install the launcher (optional)

```bash
mkdir -p ~/.local/share/applications ~/.local/bin
for size in 16 22 24 32 48 64 128 256 512; do
    src="assets/icon${size}.png"; [ "$size" = 256 ] && src="assets/icon.png"
    install -Dm644 "$src" ~/.local/share/icons/hicolor/${size}x${size}/apps/iob-zip.png
done
cp assets/iob-zip.desktop ~/.local/share/applications/io.github.iobzip.IobZip.desktop
ln -sf "$PWD/target/release/iob-zip" ~/.local/bin/iob-zip
update-desktop-database ~/.local/share/applications
```

## License notes

`sevenz-rust2` is MIT/Apache-2.0. This program does not link 7-Zip's own code,
so 7-Zip's unRAR restriction does not attach to it.

## Behavior parity with 7-Zip

Implemented by reading the real 7-Zip 26.05 FileManager source
(`CPP/7zip/UI/FileManager/`), not by guessing. Findings are recorded in
[docs-7zip-behavior.md](docs-7zip-behavior.md).

The rules that matter:

- **Double-click tries internal first**, then falls back to the OS handler —
  this is `CPanel::OpenItem(tryInternal)` returning `S_FALSE`.
- **Shift+double-click** skips the archive path entirely (`tryInternal=false`).
- **Folders sort before files in *both* directions**, because 7-Zip's
  `CompareItems` does the directory check *before* applying `_ascending`.
  This is a common thing to get wrong; there's a comment at that spot.
- **Default sort is Name, ascending** (`_sortID = kpidName`).
- Clicking the current column flips direction; clicking a new column starts
  ascending.
- A `..` row is pinned at the top of every listing.

### Interaction model & Keyboard shortcuts

Taken from the actual 7-Zip source; see [docs-7zip-behavior.md](docs-7zip-behavior.md).

- **Double-click a folder** → enter it.
- **Double-click an archive** → open it in the archive pane.
- **Double-click an archive inside an archive** → it is extracted to a temp
  folder and opened in a new iob-zip window, so nested archives never fight
  over the single archive pane.
- **Double-click a text file** → preview it natively with syntax-agnostic viewer, search & wrap.
- **Double-click an image** (PNG, JPG, BMP, GIF, WEBP) → preview it natively in-app with dimensions.
- **Double-click a PDF, video, audio, office doc** → extract a temporary copy and launch system default app.
- **Enter / Ctrl+PgDn** → open the focused item (archive, folder, or file).
- **Backspace / Ctrl+PgUp** → go up one level (parent directory).
- **Arrows / Home / End / PageUp / PageDown** → navigate rows.
- **Ctrl+Click / Shift+Click** → multi-item selection.
- **Ctrl+A** → select all items in active pane.
- **F5** → copy selected files to destination folder.
- **F7** → create new folder in current directory.
- **Delete** → confirm and delete selected items (files or folders).
- **Ctrl+D** → toggle Dark / Light theme.
- **Esc** → close preview or dialog / clear selection / cancel search.
- **Type-to-find** → typing jumps to the first matching name.
- **Drag and Drop** → drag archives or files directly into the window to open or compress.
