"""Measure the 1.12.1 outputs defects.cpp wrote and record them as observed_1121.json.

Usage: python3 -I analyze.py <fixtures dir> <defects work dir>
"""

import io
import json
import subprocess
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageCms

sys.path.insert(0, str(Path(__file__).resolve().parent))
from gen_inputs import decode_frame_index  # noqa: E402


def write(path: Path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n")


def ffprobe(path: Path):
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-show_streams", "-show_format", "-of", "json", str(path)],
        capture_output=True, text=True, check=True,
    ).stdout
    return json.loads(out)


def video_stream(probe):
    return next(s for s in probe["streams"] if s["codec_type"] == "video")


def color_tags(stream):
    return {k: stream.get(k, "unknown") for k in ("color_space", "color_primaries", "color_transfer", "color_range")}


def center_rgb(path: Path, matrix: str, rng: str):
    """Center pixel of the first frame, decoded with an explicit YUV matrix and range."""
    out = subprocess.run(
        ["ffmpeg", "-v", "error", "-i", str(path), "-frames:v", "1",
         "-vf", f"scale=in_color_matrix={matrix}:in_range={rng},format=rgb24", "-f", "rawvideo", "-"],
        capture_output=True, check=True,
    ).stdout
    stream = video_stream(ffprobe(path))
    w, h = stream["width"], stream["height"]
    frame = np.frombuffer(out, np.uint8).reshape(h, w, 3)
    return [int(v) for v in frame[h // 2, w // 2]]


def center_y(path: Path):
    """Raw luma code value of the first frame's center, before any matrix or range conversion."""
    stream = video_stream(ffprobe(path))
    w, h = stream["width"], stream["height"]
    out = subprocess.run(
        ["ffmpeg", "-v", "error", "-i", str(path), "-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "yuv420p", "-"],
        capture_output=True, check=True,
    ).stdout
    return int(np.frombuffer(out[: w * h], np.uint8).reshape(h, w)[h // 2, w // 2])


def click_times(path: Path, rate=48000):
    """Onsets of the 1 ms clicks gen_inputs writes, in seconds from the file's zero."""
    raw = subprocess.run(
        ["ffmpeg", "-v", "error", "-i", str(path), "-map", "0:a:0", "-f", "s16le", "-ac", "1", "-ar", str(rate), "-"],
        capture_output=True, check=True,
    ).stdout
    samples = np.abs(np.frombuffer(raw, np.int16).astype(np.int32))
    loud = np.flatnonzero(samples > 8000)
    onsets = []
    for index in loud:
        if not onsets or index - onsets[-1] > rate // 50:
            onsets.append(int(index))
    probe = ffprobe(path)
    audio = next(s for s in probe["streams"] if s["codec_type"] == "audio")
    start = float(audio.get("start_time", 0))
    return [round(start + i / rate, 4) for i in onsets]


SCALE_MATRIX = {"bt709": "bt709", "smpte170m": "bt601", "bt470bg": "bt601", "unknown": "auto"}
SCALE_RANGE = {"tv": "tv", "pc": "pc", "unknown": "auto"}


def analyze_d1(fixtures: Path, work: Path, process):
    runs = []
    for name in ["bt709-tv-1080p.mp4", "bt709-pc-1080p.mp4", "bt601-tv-480p.mp4"]:
        src = fixtures / "defects" / "D1" / name
        out = work / "video" / "D1" / name
        src_tags = color_tags(video_stream(ffprobe(src)))
        out_tags = color_tags(video_stream(ffprobe(out)))
        truth = center_rgb(src, SCALE_MATRIX[src_tags["color_space"]], SCALE_RANGE[src_tags["color_range"]])
        runs.append({
            "input": name,
            "source_tags": src_tags,
            "output_tags": out_tags,
            "source_center_rgb_decoded_with_its_tags": truth,
            "output_center_rgb_as_bt709_tv": center_rgb(out, "bt709", "tv"),
            "output_center_rgb_as_bt601_tv": center_rgb(out, "bt601", "tv"),
            "source_center_y": center_y(src),
            "output_center_y": center_y(out),
            "output_mode": process[f"D1/{name}"]["output_mode"],
        })
    write(fixtures / "defects" / "D1" / "observed_1121.json", {
        "pipeline": "processVideo, no detections, software encoder (libx264), defaults",
        "runs": runs,
    })


def icc_of(path: Path):
    image = Image.open(path)
    data = image.info.get("icc_profile")
    if not data:
        return None
    profile = ImageCms.ImageCmsProfile(io.BytesIO(data))
    return ImageCms.getProfileDescription(profile).strip()


def analyze_d2(fixtures: Path, work: Path):
    worker = json.loads((work / "D2" / "worker.json").read_text())
    out_dir = Path(worker["output_dir"])
    runs = []
    for src in sorted((fixtures / "defects" / "D2").glob("chart-*")):
        out = out_dir / src.name
        entry = {"input": src.name, "input_icc": icc_of(src), "output_exists": out.exists()}
        if out.exists():
            entry["output_icc"] = icc_of(out)
            a = np.asarray(Image.open(src).convert("RGB"), np.int16)
            b = np.asarray(Image.open(out).convert("RGB"), np.int16)
            entry["max_pixel_difference_vs_input"] = int(np.abs(a - b).max())
        runs.append(entry)
    write(fixtures / "defects" / "D2" / "observed_1121.json", {
        "pipeline": "ProcessorWorker, detection off, metadata not preserved",
        "outcome": worker.get("outcome"),
        "runs": runs,
    })


def magick(fmt, path):
    return subprocess.run(["magick", "identify", "-format", fmt, str(path)], capture_output=True, text=True, check=True).stdout.strip()


def webp_kind(path: Path):
    data = path.read_bytes()
    if b"VP8L" in data[:64]:
        return "lossless"
    if b"VP8 " in data[:64]:
        return "lossy"
    # VP8X files name their bitstream chunk after the extended header.
    if b"VP8L" in data:
        return "lossless"
    return "lossy" if b"VP8 " in data else "unknown"


def describe_image(path: Path):
    ext = path.suffix.lower()
    info = {"bytes": path.stat().st_size}
    if ext == ".jpg":
        info["jpeg_quality_estimate"] = int(magick("%Q", path))
        info["jpeg_sampling_factor"] = magick("%[jpeg:sampling-factor]", path)
    elif ext == ".webp":
        info["webp"] = webp_kind(path)
    elif ext == ".tif":
        info["tiff_compression"] = magick("%C", path)
    return info


def analyze_d6(fixtures: Path, work: Path):
    worker = json.loads((work / "D6" / "worker.json").read_text())
    out_dir = Path(worker["output_dir"])
    runs = []
    for src in sorted((fixtures / "defects" / "D6").iterdir()):
        if src.suffix.lower() not in (".jpg", ".webp", ".png", ".tif"):
            continue
        out = out_dir / src.name
        entry = {"input": src.name, "source": describe_image(src)}
        if out.exists():
            entry["output"] = describe_image(out)
            entry["size_ratio_output_to_input"] = round(out.stat().st_size / src.stat().st_size, 3)
            a = np.asarray(Image.open(src).convert("RGB"), np.int16)
            b = np.asarray(Image.open(out).convert("RGB"), np.int16)
            entry["max_pixel_difference_vs_input"] = int(np.abs(a - b).max())
        runs.append(entry)
    write(fixtures / "defects" / "D6" / "observed_1121.json", {
        "pipeline": "ProcessorWorker, detection off, metadata not preserved",
        "measurement": "JPEG quality is ImageMagick's quantization-table estimate",
        "runs": runs,
    })


def decode_indices(path: Path, width: int, height: int, source_w: int, source_h: int):
    raw = subprocess.run(
        ["ffmpeg", "-v", "error", "-i", str(path), "-map", "0:v:0", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
        capture_output=True, check=True,
    ).stdout
    frames = np.frombuffer(raw, np.uint8).reshape(-1, height, width, 3)
    out = []
    for frame in frames:
        if (width, height) != (source_w, source_h):
            frame = np.asarray(Image.fromarray(frame).resize((source_w, source_h), Image.Resampling.NEAREST))
        out.append(decode_frame_index(frame))
    return out


def stream_summary(path: Path):
    probe = ffprobe(path)
    v = video_stream(probe)
    a = next((s for s in probe["streams"] if s["codec_type"] == "audio"), None)
    side = v.get("side_data_list", [])
    rotation = next((d.get("rotation") for d in side if "rotation" in d), 0)
    summary = {
        "width": v["width"],
        "height": v["height"],
        "rotation": rotation,
        "sample_aspect_ratio": v.get("sample_aspect_ratio", "1:1"),
        "r_frame_rate": v["r_frame_rate"],
        "avg_frame_rate": v["avg_frame_rate"],
        "nb_frames": int(v.get("nb_frames", 0)),
        "video_start": float(v.get("start_time", 0)),
        "video_duration": float(v.get("duration", 0)),
    }
    if a:
        summary["audio_start"] = float(a.get("start_time", 0))
        summary["audio_duration"] = float(a.get("duration", 0))
    return summary


def analyze_p3(fixtures: Path, work: Path):
    selection = json.loads((work / "video" / "p3-selection.json").read_text())
    cases = {}
    for name, entry in selection.items():
        out = work / "video" / name
        probe = entry["probe"]
        sw, sh = probe["width"], probe["height"]
        if out.exists():
            summary = stream_summary(out)
            ow, oh = summary["width"], summary["height"]
            # Rotation is applied to the pixels on output, so the index blocks are read upright.
            raw_w, raw_h = (ow, oh)
            indices = decode_indices(out, raw_w, raw_h, raw_w, raw_h) if (raw_w, raw_h) == (sw, sh) else None
            if indices is None:
                rotated = []
                raw = subprocess.run(
                    ["ffmpeg", "-v", "error", "-i", str(out), "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
                    capture_output=True, check=True,
                ).stdout
                for frame in np.frombuffer(raw, np.uint8).reshape(-1, oh, ow, 3):
                    upright = np.rot90(frame, k=-1) if (ow, oh) == (sh, sw) else frame
                    rotated.append(decode_frame_index(np.ascontiguousarray(upright)))
                indices = rotated
            entry["output"] = summary
            entry["output_indices"] = indices
        src_summary = stream_summary(fixtures / "defects" / name)
        if probe["rotation"]:
            raw = subprocess.run(
                ["ffmpeg", "-v", "error", "-i", str(fixtures / "defects" / name), "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
                capture_output=True, check=True,
            ).stdout
            source_indices = [
                decode_frame_index(np.ascontiguousarray(np.rot90(f, k=-1)))
                for f in np.frombuffer(raw, np.uint8).reshape(-1, sw, sh, 3)
            ]
        else:
            source_indices = decode_indices(fixtures / "defects" / name, sw, sh, sw, sh)
        entry["source"] = src_summary
        entry["source_decoded_indices"] = source_indices
        reader = entry["analysis_reader_indices"]
        entry["preview_matches_analysis"] = all(
            v is not None and int(k) < len(reader) and reader[int(k)] == v
            for k, v in entry["preview_indices_by_frame"].items()
        )
        if "output_indices" in entry:
            entry["output_matches_analysis"] = entry["output_indices"] == reader
        cases[name] = entry
    write(fixtures / "defects" / "P-3" / "observed_1121.json", {
        "pipeline": "VideoFrameReader(960) for analysis, videoPreviewFrameArguments for review, processVideo for output",
        "frame_index_encoding": "gen_inputs.frame_index_frame / decode_frame_index",
        "cases": cases,
    })


def analyze_p5(fixtures: Path, work: Path, process):
    runs = []
    for name in ["odd-641x361-audio-longer.mp4", "audio-starts-early.mp4"]:
        src = fixtures / "defects" / "P-5" / name
        out = work / "video" / "P-5" / name
        src_summary = stream_summary(src)
        out_summary = stream_summary(out)
        runs.append({
            "input": name,
            "source": src_summary,
            "output": out_summary,
            "output_indices": decode_indices(out, out_summary["width"], out_summary["height"], out_summary["width"], out_summary["height"]),
            "source_click_times": click_times(src),
            "output_click_times": click_times(out),
            "process": {k: process[f"P-5/{name}"][k] for k in ("status", "frame_count")},
        })
    write(fixtures / "defects" / "P-5" / "observed_1121.json", {
        "pipeline": "processVideo, no detections, software encoder, keep audio",
        "click_times": "seconds on each file's own timeline; clicks are every 0.5 s of the source audio",
        "runs": runs,
    })


def analyze_p1(fixtures: Path, work: Path):
    video = json.loads((work / "video" / "p1-video.json").read_text())
    photo = json.loads((work / "P-1-photo.json").read_text())
    out = Path(video["output"])
    source = fixtures / "defects" / "P-3" / "cfr-2997-offset-bframes.mp4"
    exposed = []
    if out.exists():
        def frames(path):
            raw = subprocess.run(
                ["ffmpeg", "-v", "error", "-i", str(path), "-f", "rawvideo", "-pix_fmt", "rgb24", "-"],
                capture_output=True, check=True,
            ).stdout
            return np.frombuffer(raw, np.uint8).reshape(-1, 360, 640, 3).astype(np.int16)

        a = frames(source)
        b = frames(out)
        region = (slice(150, 230), slice(400, 480))
        for index in range(min(len(a), len(b))):
            diff = float(np.abs(a[index][region] - b[index][region]).mean())
            exposed.append(round(diff, 2))
    video["log"] = None
    write(fixtures / "defects" / "P-1" / "observed_1121.json", {
        "video": {
            "scenario": video["scenario"],
            "status": video["status"],
            "uncovered_spans": video["uncovered_spans"],
            "uncovered_frames": video["uncovered_frames"],
            "output_published": video["output_exists"],
            "mean_abs_difference_source_vs_output_in_face_region_per_frame": exposed,
            "note": "a value near 0 means the region left the pipeline unmasked",
        },
        "photo": {
            "scenario": photo["scenario"],
            "outcome": photo["outcome"],
            "summary": photo.get("summary"),
            "output_published": photo["output_exists"],
            "warning_lines": [line for line in photo["log"] if "Warning" in line],
        },
    })


def exif_tags(path: Path):
    out = subprocess.run(["exiftool", "-j", "-G1", "-a", "-n", "-s", str(path)], capture_output=True, text=True, check=True).stdout
    tags = json.loads(out)[0]
    drop = {"SourceFile"}
    return {k: v for k, v in sorted(tags.items()) if k not in drop and not k.startswith(("System:", "File:", "ExifTool:", "Composite:"))}


def analyze_metadata(fixtures: Path, work: Path):
    copy = json.loads((work / "metadata" / "copy.json").read_text())
    outputs = {}
    for name, ok in copy.items():
        outputs[name] = {"copy_metadata_returned": ok, "tags": exif_tags(work / "metadata" / name)}
    write(fixtures / "metadata" / "observed_1121.json", {
        "function": "copyMetadata(source, freshly encoded JPEG, normalizeOrientation=true, keepLocation)",
        "tool": "exiftool -G1 -a -n",
        "inputs": {
            "rich-exif.jpg": exif_tags(fixtures / "metadata" / "rich-exif.jpg"),
            "oversized-entry.jpg": exif_tags(fixtures / "metadata" / "oversized-entry.jpg"),
        },
        "outputs": outputs,
    })


def main():
    fixtures = Path(sys.argv[1])
    work = Path(sys.argv[2])
    process = json.loads((work / "video" / "process.json").read_text())
    analyze_d1(fixtures, work, process)
    analyze_d2(fixtures, work)
    analyze_d6(fixtures, work)
    analyze_p1(fixtures, work)
    analyze_p3(fixtures, work)
    analyze_p5(fixtures, work, process)
    analyze_metadata(fixtures, work)


if __name__ == "__main__":
    main()
