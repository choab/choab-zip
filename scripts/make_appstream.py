#!/usr/bin/env python3
"""Compose the AppStream catalogue entry Flatpak expects.

`flatpak build-export --update-appstream` wants a *composed* document in
`files/share/app-info/xmls/<app-id>.xml.gz`: the same component as the
metainfo file, wrapped in `<components>` and enriched with cached icon sizes
and the media types advertised by the desktop entry.

    ./scripts/make_appstream.py OUT.xml.gz

The id, name and launchable are taken from the metainfo file, so renaming the
application only means editing the metainfo.
"""

import gzip
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
METAINFO = ROOT / "packaging/metainfo/io.github.iobzip.IobZip.metainfo.xml"
DESKTOP = ROOT / "packaging/flatpak/io.github.iobzip.IobZip.desktop"


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    out = pathlib.Path(sys.argv[1])

    xml = METAINFO.read_text()
    component = xml.split("?>", 1)[1].strip()

    app_id = re.search(r"<id>([^<]+)</id>", component).group(1)

    desktop = DESKTOP.read_text()
    mime_line = next(
        (line for line in desktop.splitlines() if line.startswith("MimeType=")), ""
    )
    mime = [m for m in mime_line.partition("=")[2].split(";") if m]

    icons = (
        f'  <icon type="cached" width="64" height="64">{app_id}.png</icon>\n'
        f'  <icon type="cached" width="128" height="128">{app_id}.png</icon>\n'
        f'  <icon type="stock">{app_id}</icon>'
    )
    component = component.replace(
        f'<launchable type="desktop-id">{app_id}.desktop</launchable>',
        f'<launchable type="desktop-id">{app_id}.desktop</launchable>\n{icons}',
    )

    if mime:
        provides = "\n".join(f"    <mediatype>{m}</mediatype>" for m in mime)
        component = component.replace(
            "</categories>", f"</categories>\n  <provides>\n{provides}\n  </provides>"
        )

    composed = (
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<components version="1.2">\n'
        f"{component}\n"
        "</components>\n"
    )

    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(gzip.compress(composed.encode(), 9, mtime=0))
    print(f"wrote {out} ({app_id})")


if __name__ == "__main__":
    main()
