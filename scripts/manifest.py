#!/usr/bin/env python3
"""Write archive/videos/MANIFEST.csv: one row per downloaded video.

Rows come from yt-dlp *.info.json files (playlist/search container JSONs are skipped). Media files
without an info.json (archive.org items, e.g. archive/videos/ia/tweakers*/) get a row with the
archive.org identifier as id and whatever ffprobe-free metadata the path provides.
"""
import csv
import hashlib
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VIDEOS = ROOT / "archive" / "videos"
MEDIA_EXT = (".mkv", ".mp4", ".webm", ".flv", ".mov", ".avi", ".mp3")
FIELDS = ["path", "section", "id", "title", "channel", "upload_date", "duration", "sha256"]


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def media_for(info_path: Path) -> Path | None:
    stem = info_path.name.removesuffix(".info.json")
    for ext in MEDIA_EXT:
        candidate = info_path.with_name(stem + ext)
        if candidate.exists():
            return candidate
    return None


def section_of(path: Path) -> str:
    return path.relative_to(VIDEOS).parts[0]


def main() -> int:
    rows = []
    claimed: set[Path] = set()
    for info_path in sorted(VIDEOS.rglob("*.info.json")):
        info = json.loads(info_path.read_text(encoding="utf-8"))
        if info.get("_type") == "playlist":
            continue
        media = media_for(info_path)
        if media:
            claimed.add(media)
        rows.append({
            "path": str((media or info_path).relative_to(ROOT)),
            "section": section_of(info_path),
            "id": info.get("id", ""),
            "title": info.get("title", ""),
            "channel": info.get("channel") or info.get("uploader") or "",
            "upload_date": info.get("upload_date", ""),
            "duration": info.get("duration", ""),
            "sha256": sha256(media) if media else "",
        })
    # yt-dlp leaves intermediate .fNNN streams only on failed merges; they are not videos of their own.
    for media in sorted(p for p in VIDEOS.rglob("*") if p.suffix in MEDIA_EXT and ".f" not in p.stem[-6:]):
        if media in claimed or media.with_suffix(".mkv") in claimed:
            continue
        rows.append({
            "path": str(media.relative_to(ROOT)),
            "section": section_of(media),
            "id": media.parent.name,
            "title": media.stem,
            "channel": "archive.org",
            "upload_date": "",
            "duration": "",
            "sha256": sha256(media),
        })
    out = VIDEOS / "MANIFEST.csv"
    with out.open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=FIELDS)
        writer.writeheader()
        writer.writerows(rows)
    missing = sum(1 for r in rows if not r["sha256"])
    print(f"{out.relative_to(ROOT)}: {len(rows)} rows, {missing} without media", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
