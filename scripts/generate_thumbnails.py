#!/usr/bin/env python3
"""
Generate or extract thumbnails for all videos in a directory.

This script scans for video files (.mp4, .mkv, .webm, etc.):
1. Checks for and extracts any embedded thumbnails (MKV attachments like cover.webp/cover.jpg,
   or MP4/MKV attached_pic cover streams).
2. If no embedded thumbnail exists, captures a frame at 1s (or 5% duration).
3. Saves the thumbnail as <video_stem>.<ext> in the same folder as the video,
   so yt-dlp-gui and file managers immediately detect and display it.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor, as_completed

VIDEO_EXTENSIONS = {".mp4", ".mkv", ".webm", ".mov", ".m4v", ".avi", ".flv", ".ts"}
IMAGE_EXTENSIONS = {".jpg", ".jpeg", ".png", ".webp"}


def get_existing_thumbnail(video_path: Path) -> Path | None:
    parent = video_path.parent
    stem = video_path.stem
    for ext in [".webp", ".jpg", ".jpeg", ".png"]:
        candidate = parent / f"{stem}{ext}"
        if candidate.is_file() and candidate.stat().st_size > 0:
            return candidate
    return None


def probe_streams(video_path: Path, ffprobe_bin: str) -> list[dict]:
    cmd = [
        ffprobe_bin,
        "-v", "error",
        "-show_entries", "stream=index,codec_name,codec_type,disposition:stream_tags=filename,mimetype",
        "-of", "json",
        str(video_path),
    ]
    try:
        res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=True)
        data = json.loads(res.stdout)
        return data.get("streams", [])
    except Exception:
        return []


def extract_embedded_thumbnail(video_path: Path, streams: list[dict], ffmpeg_bin: str) -> Path | None:
    stem = video_path.stem
    parent = video_path.parent

    # 1. Check MKV / WebM attachments (e.g. cover.webp, cover.jpg)
    attachments = [s for s in streams if s.get("codec_type") == "attachment"]
    for attach_idx, s in enumerate(attachments):
        tags = s.get("tags") or {}
        orig_filename = tags.get("filename", "")
        mimetype = tags.get("mimetype", "")

        ext = ".jpg"
        if ".webp" in orig_filename.lower() or "webp" in mimetype.lower():
            ext = ".webp"
        elif ".png" in orig_filename.lower() or "png" in mimetype.lower():
            ext = ".png"

        target_thumb = parent / f"{stem}{ext}"
        cmd = [
            ffmpeg_bin,
            "-y",
            f"-dump_attachment:t:{attach_idx}",
            str(target_thumb),
            "-t", "0",
            "-i", str(video_path),
            "-f", "null",
            "-",
        ]
        try:
            res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15)
            if target_thumb.is_file() and target_thumb.stat().st_size > 100:
                return target_thumb
            target_thumb.unlink(missing_ok=True)
        except Exception:
            target_thumb.unlink(missing_ok=True)

    # 2. Check attached_pic streams (MP4/MKV cover art)
    for idx, s in enumerate(streams):
        disposition = s.get("disposition") or {}
        if disposition.get("attached_pic") == 1:
            stream_idx = s.get("index", idx)
            target_thumb = parent / f"{stem}.jpg"
            cmd = [
                ffmpeg_bin,
                "-y",
                "-i", str(video_path),
                "-map", f"0:{stream_idx}",
                "-c", "copy",
                str(target_thumb),
            ]
            try:
                res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15)
                if target_thumb.is_file() and target_thumb.stat().st_size > 100:
                    return target_thumb
                target_thumb.unlink(missing_ok=True)
            except Exception:
                target_thumb.unlink(missing_ok=True)

    return None


def generate_frame_thumbnail(video_path: Path, ffmpeg_bin: str, time_sec: float = 1.0) -> Path | None:
    stem = video_path.stem
    parent = video_path.parent
    target_thumb = parent / f"{stem}.jpg"

    cmd = [
        ffmpeg_bin,
        "-y",
        "-ss", str(time_sec),
        "-i", str(video_path),
        "-vframes", "1",
        "-q:v", "2",
        str(target_thumb),
    ]
    try:
        res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=20)
        if target_thumb.is_file() and target_thumb.stat().st_size > 100:
            return target_thumb

        # Fallback to 0.0s if 1.0s failed (e.g. video shorter than 1s)
        if time_sec > 0.1:
            cmd[2] = "00:00:00.1"
            subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=20)
            if target_thumb.is_file() and target_thumb.stat().st_size > 100:
                return target_thumb

        target_thumb.unlink(missing_ok=True)
    except Exception:
        target_thumb.unlink(missing_ok=True)

    return None


def process_video(video_path: Path, ffmpeg_bin: str, ffprobe_bin: str, overwrite: bool = False) -> tuple[Path, str, str]:
    existing = get_existing_thumbnail(video_path)
    if existing and not overwrite:
        return video_path, "skipped", f"Already exists: {existing.name}"

    streams = probe_streams(video_path, ffprobe_bin)

    # Try embedded thumbnail extraction first
    embedded = extract_embedded_thumbnail(video_path, streams, ffmpeg_bin)
    if embedded:
        # If an older frame capture jpg exists and we extracted webp, clean up the jpg
        if embedded.suffix != ".jpg":
            (video_path.parent / f"{video_path.stem}.jpg").unlink(missing_ok=True)
        return video_path, "embedded", f"Extracted embedded thumbnail -> {embedded.name}"

    # Fallback to frame capture
    frame = generate_frame_thumbnail(video_path, ffmpeg_bin)
    if frame:
        return video_path, "frame", f"Captured video frame -> {frame.name}"

    return video_path, "failed", "Could not extract or generate thumbnail"


def main() -> int:
    parser = argparse.ArgumentParser(description="Extract or generate thumbnails for local videos.")
    parser.add_argument(
        "directory",
        nargs="?",
        default="/run/media/paul/Seagate Portable Drive/video_manager",
        help="Target directory to scan (default: /run/media/paul/Seagate Portable Drive/video_manager)",
    )
    parser.add_argument("--overwrite", action="store_true", help="Overwrite existing thumbnails")
    parser.add_argument("--threads", type=int, default=4, help="Number of concurrent worker threads (default: 4)")
    args = parser.parse_args()

    target_dir = Path(args.directory).resolve()
    if not target_dir.is_dir():
        print(f"Error: Directory does not exist: {target_dir}", file=sys.stderr)
        return 1

    ffmpeg_bin = shutil.which("ffmpeg") or "ffmpeg"
    ffprobe_bin = shutil.which("ffprobe") or "ffprobe"

    print(f"Scanning '{target_dir}' for videos...")
    video_files: list[Path] = []
    for root, _, files in os.walk(target_dir):
        for f in files:
            p = Path(root) / f
            if p.suffix.lower() in VIDEO_EXTENSIONS:
                video_files.append(p)

    print(f"Found {len(video_files)} video file(s).")
    if not video_files:
        return 0

    counts = {"embedded": 0, "frame": 0, "skipped": 0, "failed": 0}

    with ThreadPoolExecutor(max_workers=args.threads) as executor:
        futures = {
            executor.submit(process_video, v, ffmpeg_bin, ffprobe_bin, args.overwrite): v
            for v in video_files
        }
        for future in as_completed(futures):
            video = futures[future]
            try:
                path, status, msg = future.result()
                counts[status] = counts.get(status, 0) + 1
                prefix = {
                    "embedded": "[EMBEDDED]",
                    "frame": "[FRAME]   ",
                    "skipped": "[SKIP]    ",
                    "failed": "[FAIL]    ",
                }.get(status, "[INFO]    ")
                print(f"{prefix} {video.name} - {msg}")
            except Exception as err:
                counts["failed"] = counts.get("failed", 0) + 1
                print(f"[FAIL]     {video.name} - Exception: {err}")

    print("\n----------------------------------------")
    print(f"Total: {len(video_files)} | Embedded Extracted: {counts['embedded']} | Frames Captured: {counts['frame']} | Skipped: {counts['skipped']} | Failed: {counts['failed']}")
    print("Done!")
    return 0


if __name__ == "__main__":
    sys.exit(main())
