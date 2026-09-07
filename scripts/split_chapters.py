#!/usr/bin/env python3
"""Split a video into chapter files using ffprobe and GPU-assisted ffmpeg."""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any


INVALID_FILENAME_CHARS = re.compile(r'[<>:"/\\|?*\x00-\x1f]')


def executable(value: str) -> str:
    path = Path(value)
    if path.is_file():
        return str(path)
    return shutil.which(value) or value


def run_ffprobe(ffprobe: str, input_path: Path) -> list[dict[str, Any]]:
    result = subprocess.run(
        [
            executable(ffprobe),
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_chapters",
            str(input_path),
        ],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or "ffprobe failed")
    try:
        data = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"Could not parse ffprobe output: {error}") from error
    return data.get("chapters", [])


def safe_title(title: str, fallback: str) -> str:
    cleaned = INVALID_FILENAME_CHARS.sub("_", title).strip(" .")
    return cleaned or fallback


def output_path(output_dir: Path, number: int, title: str, suffix: str) -> Path:
    base = output_dir / f"{number:03d} - {safe_title(title, f'Chapter {number}')}{suffix}"
    candidate = base
    counter = 2
    while candidate.exists():
        candidate = base.with_name(f"{base.stem} ({counter}){base.suffix}")
        counter += 1
    return candidate


def cut_chapter(
    ffmpeg: str,
    input_path: Path,
    output: Path,
    start: float,
    end: float,
    cq: int,
    codec: str,
) -> None:
    command = [
        executable(ffmpeg),
        "-y",
        "-loglevel",
        "error",
        "-hwaccel",
        "cuda",
        "-ss",
        str(start),
        "-to",
        str(end),
        "-i",
        str(input_path),
        "-map",
        "0",
        "-c:v",
        codec,
        "-preset",
        "p4",
        "-rc",
        "vbr",
        "-cq",
        str(cq),
        "-spatial-aq",
        "1",
        "-temporal-aq",
        "1",
        "-forced-idr",
        "1",
        "-c:a",
        "copy",
        "-map_chapters",
        "-1",
        str(output),
    ]
    subprocess.run(command, check=True)


def split_video(
    input_path: Path,
    ffprobe: str,
    ffmpeg: str,
    cq: int,
    codec: str,
) -> int:
    if not input_path.is_file():
        print(f"Input file not found: {input_path}", file=sys.stderr)
        return 1

    print(f"Checking embedded chapters: {input_path}")
    chapters = run_ffprobe(ffprobe, input_path)
    valid_chapters = []
    for chapter in chapters:
        try:
            start = float(chapter["start_time"])
            end = float(chapter["end_time"])
        except (KeyError, TypeError, ValueError):
            continue
        if end > start:
            valid_chapters.append((chapter, start, end))

    if not valid_chapters:
        print("No embedded chapters found; nothing to cut.")
        return 0

    output_dir = input_path.parent / f"{input_path.stem}_chapters"
    output_dir.mkdir(exist_ok=True)
    print(f"Found {len(valid_chapters)} chapter(s). Output: {output_dir}")

    for number, (chapter, start, end) in enumerate(valid_chapters, 1):
        title = str(chapter.get("tags", {}).get("title", f"Chapter {number}"))
        output = output_path(output_dir, number, title, input_path.suffix or ".mkv")
        print(f"[{number}/{len(valid_chapters)}] {title} ({start:.3f}s - {end:.3f}s)")
        try:
            cut_chapter(ffmpeg, input_path, output, start, end, cq, codec)
        except subprocess.CalledProcessError as error:
            print(f"Failed to cut chapter {number}: ffmpeg exited with {error.returncode}", file=sys.stderr)
            return error.returncode or 1

    print("Chapter cutting complete.")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path, help="video file containing embedded chapters")
    parser.add_argument("--ffprobe", default="ffprobe", help="ffprobe executable path")
    parser.add_argument("--ffmpeg", default="ffmpeg", help="ffmpeg executable path")
    parser.add_argument("--cq", type=int, default=23, help="NVENC constant quality value")
    parser.add_argument("--codec", default="hevc_nvenc", help="NVENC video codec")
    args = parser.parse_args()

    try:
        return split_video(args.input, args.ffprobe, args.ffmpeg, args.cq, args.codec)
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"Chapter cutting failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
