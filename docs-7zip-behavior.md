# 7-Zip FileManager behavior spec (from real source)

Source: `7z2605-src.tar.xz` (github.com/ip7z/7zip, tag 26.05),
`CPP/7zip/UI/FileManager/`. Notes gathered by reading the actual code,
not from documentation.

## Item activation — PanelListNotify.cpp
```cpp
void CPanel::OnNotifyActivateItems() {
  bool alt = IsKeyDown(VK_MENU), ctrl = IsKeyDown(VK_CONTROL),
       shift = IsKeyDown(VK_SHIFT);
  if (!shift && alt && !ctrl) Properties();      // Alt+double-click
  else OpenSelectedItems(!shift || alt || ctrl); // tryInternal
}
```
- **Double-click** → open. `tryInternal = !shift` → tries to open as an archive
  first; **Shift+double-click** goes straight to the external app.
- **Alt+double-click** → Properties dialog.
- Enter / NM_RETURN behaves exactly like double-click (same handler).

## Open order — PanelItemOpen.cpp
```cpp
void CPanel::OpenItem(index, tryInternal, tryExternal, type)
```
1. If inside an archive → `OpenItemInArchive(...)`.
2. Else if `tryInternal` → `OpenAsArc_Index()` (open as archive).
   - `S_OK`/`E_ABORT` → done.
   - `S_FALSE` → not an archive, fall through.
3. Else `StartApplicationDontWait()` — hand to the OS default app.

So: **archive first, system app as fallback.** Never the reverse.

## Selection — PanelSelect.cpp
- Single click selects; Ctrl+click toggles; Shift+click range-selects from
  `_startGroupSelect`.
- `AlternativeSelection` option (`_mySelectMode`) enables click-to-select
  without Ctrl.

## Sorting — PanelSort.cpp (CompareItems)
```cpp
if (isDir1 != isDir2) return isDir1 ? -1 : 1;   // BEFORE the ascending flip
...
return panel->_ascending ? res : -res;
```
- **Folders always precede files, in both ascending and descending.**
  The dir check happens before the `ascending` inversion.
- Sort key defaults to `kpidName`, ascending (Panel.cpp:387).
- Ties fall back to name, then original load order.
- `..` parent row is always pinned first (`lParam == kParentIndex → -1`).

## Columns — PanelItems.cpp
- Default width 100px; **Name is 160px**.
- Text-valued columns left-aligned; numeric right-aligned.
- FS folder hides: `kpidATime, kpidChangeTime, kpidAttrib, kpidPackSize,
  kpidINode, kpidLinks, kpidNtReparse`.
  → in a plain file listing you see: **Name, Size, Modified**.
  Inside an archive `PackSize` is shown too.

## Drag & drop — PanelDrag.cpp
- Custom clipboard format `"7-Zip::SetTargetFolder"` carries the destination
  panel path so a drop can copy between panes.
- Drop target registered on the panel HWND (`RegisterDragDrop`).

## Keyboard — Panel.cpp
- `Ctrl+PgDn` → open focused item **as internal archive**.
- `Ctrl+PgUp` → go to parent folder.
- `/`, `\` or `/` → open drives folder.
