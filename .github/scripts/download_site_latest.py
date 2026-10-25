"""Writes download.tuken.site's latest.json for a release.

It is the release's own latest.json with every link moved to the download
site, plus each file's sha256 and size (install.ps1 checks them) and a
`downloads` entry per system, which the website's download buttons read.

    python3 download_site_latest.py <folder with the release's files> <tag>
"""

import hashlib
import json
import pathlib
import re
import sys
from urllib.parse import quote, unquote

SITE = "https://download.tuken.site"
# The installer people download, per system.
DOWNLOADS = {"windows": "*_x64-setup.exe", "macos": "*_universal.dmg"}


def main(folder: str, tag: str) -> None:
    if not re.fullmatch(r"v\d+\.\d+\.\d+", tag):
        sys.exit(f"unexpected tag {tag!r}")
    root = pathlib.Path(folder)
    latest = json.loads((root / "latest.json").read_text(encoding="utf-8"))
    if latest["version"] != tag[1:]:
        sys.exit(f"latest.json is for {latest['version']}, the tag is {tag}")

    def entry(name: str) -> dict:
        data = (root / name).read_bytes()
        return {
            "url": f"{SITE}/releases/{tag}/{quote(name)}",
            "sha256": hashlib.sha256(data).hexdigest(),
            "size": len(data),
        }

    for key, platform in latest["platforms"].items():
        name = unquote(platform["url"].rsplit("/", 1)[1])
        sig = (root / f"{name}.sig").read_text(encoding="utf-8").strip()
        if platform["signature"].strip() != sig:
            sys.exit(f"{key}: the signature differs from {name}.sig")
        platform.update(entry(name))

    downloads = {}
    for system, pattern in DOWNLOADS.items():
        found = sorted(root.glob(pattern))
        if len(found) > 1:
            sys.exit(f"more than one {pattern}: {[f.name for f in found]}")
        if found:
            downloads[system] = entry(found[0].name)
    if "windows" not in downloads:
        sys.exit("the release has no Windows installer")
    latest["downloads"] = downloads

    sys.stdout.reconfigure(encoding="utf-8")
    json.dump(latest, sys.stdout, ensure_ascii=False, indent=2)
    print()


if __name__ == "__main__":
    main(*sys.argv[1:])
