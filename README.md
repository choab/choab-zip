# 7rust-zip

A native, 7-Zip-style archiver for Linux, written in pure Rust.

No Wine, no Qt, no `7z` subprocess. One ~11 MB self-contained binary.

## Why

7-Zip's interface is clean and predictable; most Linux archive managers are
not. The engine side of 7-Zip is already excellent — what's missing on Linux
is a fast, native two-pane file browser in that style. That's all this is.

## Features

| Feature | Status |
|---|---|
| 7z / zip / tar / tar.gz / tar.xz / tar.bz2 / tar.zst | list, extract, create |
| gz / xz / bz2 single-file | extract |
| **Browse inside archives** (folders, `..`, nested dirs) | done |
| **Built-in file viewer** (text + hex, in-app) | done |
| **Integrity test** with CRC verification | done |
| Light theme, Win32-like | done |
| Two-pane layout, like 7zFM | done |
| Clickable column sort, folders-first both ways | done |
| Keyboard: arrows, Home/End, Enter, Backspace, Ctrl+PgUp/PgDn | done |
| Type-to-find | done |
| Drag and drop between panes | not yet |
| AES-256 encrypted 7z | engine ready, no UI yet |
| RAR | read-only listings; unRAR licence forbids reuse |

### No external programs — by design

The app **never** launches another program. Double-clicking a non-archive does
not call `xdg-open`; it opens the built-in viewer instead. `grep -r xdg-open
src/` returns nothing, and there is no `Command::new` anywhere in the source.

## Build

```bash
cargo build --release
./target/release/7rust-zip
```

Requires a Rust toolchain and X11/Wayland dev headers (for `eframe`).

## Verify the engine without a display

```bash
./target/release/7rust-zip --selftest path/to/archive.7z
```

Lists entries and extracts to a temp dir. Useful for CI and for checking
that a given codec works on your machine.

## Layout

- `src/engine.rs` — format detection, listing, extract, create. All archive
  logic lives here and is UI-agnostic, so it can be reused or tested alone.
- `src/main.rs` — the egui front-end (panels, browser, worker threads).
- `assets/icon.png` — window/launcher icon, embedded with `include_bytes!`.
  Generated from the hand-made logo `assets/7rz.logo.png`.
- `assets/7rust-zip.desktop` — launcher entry for the application menu.
- `scripts/make_logo_icons.py` — derives the app icon (full logo) and the
  titlebar/archive-row "7" badge from `assets/7rz.logo.png`.
- `scripts/generate_all_icons.py` — draws the toolbar, row and window-control
  icon set (Pycairo) into `assets/icons/`.

## Icon guidance (IMPORTANT)

egui only bundles `Ubuntu-Light`, `Hack`, `NotoEmoji` and a **subset** of
`emoji-icon-font`. Most emoji are **not** included, so characters such as
📁 📄 📂 📦 🗀 render as tofu boxes (`□`).

Glyphs verified present and safe to use:

```
★ ☆ ▶ ■ □ ● ✔ ✚ ⏏ ⚙ ☰ ✕ ✖ ⬆ ⬇ ⟳ ▣ · • … » ›
```

Do **not** use: 📁 📄 📂 📤 🗁 🗋 🖹 🔍 ▸ ▾ ▪ ◇ ◆ ↑ → ↩ ⌂ and the
box-drawing set (─ │ ├ └). To use a wider set, load a font with
`egui::Context::set_fonts`.

## Install the launcher (optional)

```bash
mkdir -p ~/.local/share/applications ~/.local/share/icons/hicolor/256x256/apps ~/.local/bin
cp assets/icon.png ~/.local/share/icons/hicolor/256x256/apps/7rust-zip.png
cp assets/7rust-zip.desktop ~/.local/share/applications/
ln -sf "$PWD/target/release/7rust-zip" ~/.local/bin/7rust-zip
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

### Not implemented yet

- Alt+double-click opens Properties in 7-Zip; here it only avoids opening.
- Drag and drop between panes (7-Zip uses a custom
  `"7-Zip::SetTargetFolder"` clipboard format).
- `Ctrl+PgDn` (open as internal archive) / `Ctrl+PgUp` (parent).
- Keyboard navigation (arrows, Home/End, type-to-find).

## Using it

```bash
./target/release/7rust-zip              # open the file browser
./target/release/7rust-zip a.7z         # open with an archive loaded
./target/release/7rust-zip --selftest a.7z   # headless list + extract
```

`samples/` contains a few archives made by the real 7-Zip, for testing.

### Interaction model

Taken from the actual 7-Zip source; see [docs-7zip-behavior.md](docs-7zip-behavior.md).

- **Double-click a folder** → enter it.
- **Double-click an archive** → open it in the right pane.
- **Double-click a file inside an archive** → preview it in the built-in viewer.
- **Double-click a plain file on disk** → preview it (never handed to another app).
- **Backspace / Ctrl+PgUp** → go up one level.
- **Enter / Ctrl+PgDn** → open the focused item.
- **Arrows / Home / End** → move the selection.
- **Typing** jumps to the first matching name.
