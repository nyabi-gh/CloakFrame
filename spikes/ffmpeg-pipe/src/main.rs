//! M0 spike: raw-frame pipe throughput between the engine and external FFmpeg processes
//! (plan 4.4), for 1080p and 4K.
//!
//!   ffmpeg-pipe-spike gen <dir>           make the 10 s test clips
//!   ffmpeg-pipe-spike bench <dir> [--hw-decode videotoolbox|d3d11va|cuda|vaapi|qsv|none]
//!                     [--hw-encoders h264_videotoolbox,hevc_videotoolbox] [clip names]
//!                                         run the measurements, one JSON line each
//!   ffmpeg-pipe-spike sink                internal: read stdin to EOF (raw pipe ceiling)

use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Instant;

use anyhow::{Context, Result, bail, ensure};

const FPS: u32 = 30;
const SECONDS: u32 = 10;
const FRAMES: usize = (FPS * SECONDS) as usize;

struct Clip {
    name: &'static str,
    width: usize,
    height: usize,
    codec: &'static str,
}

const CLIPS: [Clip; 3] = [
    Clip { name: "1080p30-h264", width: 1920, height: 1080, codec: "libx264" },
    Clip { name: "2160p30-h264", width: 3840, height: 2160, codec: "libx264" },
    Clip { name: "2160p30-hevc", width: 3840, height: 2160, codec: "libx265" },
];

fn ffmpeg() -> Command {
    let mut c = Command::new("ffmpeg");
    c.args(["-hide_banner", "-v", "error", "-nostdin", "-y"]);
    c
}

fn generate(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    for clip in &CLIPS {
        let out = dir.join(format!("{}.mp4", clip.name));
        // testsrc2 with temporal noise and a moving overlay: busy enough that decode cost is in
        // the range of camera footage (tens of Mbit/s at 4K), unlike a static test card.
        let src = format!(
            "testsrc2=size={w}x{h}:rate={FPS},noise=alls=12:allf=t+u,format=yuv420p",
            w = clip.width,
            h = clip.height
        );
        let mut cmd = ffmpeg();
        cmd.args(["-f", "lavfi", "-i", &src, "-t", &SECONDS.to_string()]);
        cmd.args(["-c:v", clip.codec, "-preset", "fast"]);
        if clip.codec == "libx265" {
            cmd.args(["-crf", "24", "-tag:v", "hvc1", "-x265-params", "log-level=error"]);
        } else {
            cmd.args(["-crf", "20"]);
        }
        cmd.args(["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709", "-color_range", "tv"]);
        cmd.arg(&out);
        let status = cmd.status()?;
        ensure!(status.success(), "ffmpeg failed for {}", clip.name);
        let probe = Command::new("ffprobe")
            .args(["-v", "error", "-show_entries", "format=bit_rate,size", "-of", "csv=p=0"])
            .arg(&out)
            .output()?;
        println!("{}: {}", out.display(), String::from_utf8_lossy(&probe.stdout).trim());
    }
    Ok(())
}

#[cfg(not(unix))]
fn child_cpu_seconds() -> f64 {
    0.0
}

#[cfg(not(unix))]
fn self_cpu_seconds() -> f64 {
    0.0
}

#[cfg(unix)]
fn child_cpu_seconds() -> f64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_CHILDREN, &mut usage) };
    let t = |tv: libc::timeval| tv.tv_sec as f64 + tv.tv_usec as f64 / 1e6;
    t(usage.ru_utime) + t(usage.ru_stime)
}

#[cfg(unix)]
fn self_cpu_seconds() -> f64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    let t = |tv: libc::timeval| tv.tv_sec as f64 + tv.tv_usec as f64 / 1e6;
    t(usage.ru_utime) + t(usage.ru_stime)
}

struct Measure {
    started: Instant,
    child_cpu: f64,
    self_cpu: f64,
}

impl Measure {
    fn start() -> Self {
        Self { started: Instant::now(), child_cpu: child_cpu_seconds(), self_cpu: self_cpu_seconds() }
    }

    fn report(self, label: &str, clip: &str, frames: usize, bytes_per_frame: usize, extra: serde_json::Value) {
        let wall = self.started.elapsed().as_secs_f64();
        let line = serde_json::json!({
            "case": label,
            "clip": clip,
            "frames": frames,
            "wall_s": wall,
            "fps": frames as f64 / wall,
            "realtime_x": frames as f64 / wall / FPS as f64,
            "pipe_mb_s": if bytes_per_frame > 0 { frames as f64 * bytes_per_frame as f64 / wall / 1e6 } else { 0.0 },
            "ffmpeg_cpu_s": child_cpu_seconds() - self.child_cpu,
            "spike_cpu_s": self_cpu_seconds() - self.self_cpu,
            "extra": extra,
        });
        println!("{line}");
    }
}

fn decoder(path: &Path, hw: Option<&str>, scale: Option<(usize, usize)>, pix_fmt: &str) -> Command {
    decoder_limited(path, hw, scale, pix_fmt, None)
}

fn decoder_limited(path: &Path, hw: Option<&str>, scale: Option<(usize, usize)>, pix_fmt: &str, limit: Option<usize>) -> Command {
    let mut cmd = ffmpeg();
    cmd.arg("-xerror");
    if let Some(hwaccel) = hw {
        cmd.args(["-hwaccel", hwaccel]);
    }
    cmd.args(["-protocol_whitelist", "file", "-i"]).arg(format!("file:{}", path.display()));
    // 1.12.1's reader filter, plus the analysis downscale when asked for.
    let mut filter = format!("setpts=PTS-STARTPTS,fps=fps={FPS}/1:start_time=0");
    if let Some((w, h)) = scale {
        filter.push_str(&format!(",scale={w}:{h}:flags=area"));
    }
    cmd.args(["-map", "0:v:0", "-vf", &filter]);
    if let Some(n) = limit {
        cmd.args(["-frames:v", &n.to_string()]);
    }
    cmd.args(["-f", "rawvideo", "-pix_fmt", pix_fmt, "-"]);
    cmd.stdout(Stdio::piped());
    cmd
}

fn encoder(out: &Path, width: usize, height: usize, pix_fmt: &str, codec: &str, tagged: bool) -> Command {
    let vaapi = codec.ends_with("_vaapi");
    let mut cmd = ffmpeg();
    if vaapi {
        cmd.args(["-init_hw_device", "vaapi=va:/dev/dri/renderD128", "-filter_hw_device", "va"]);
    }
    cmd.args(["-f", "rawvideo", "-pix_fmt", pix_fmt, "-s", &format!("{width}x{height}"), "-framerate", &FPS.to_string()]);
    cmd.args(["-i", "-", "-map", "0:v:0"]);
    match codec {
        "libx264" => cmd.args(["-c:v", "libx264", "-preset", "medium", "-crf", "18"]),
        // Quality settings as 1.12.1 derives them from crf 18 (VideoIo.cpp videoEncoderArgs).
        "h264_videotoolbox" => cmd.args(["-c:v", "h264_videotoolbox", "-q:v", "64"]),
        "hevc_videotoolbox" => cmd.args(["-c:v", "hevc_videotoolbox", "-q:v", "64", "-tag:v", "hvc1"]),
        e if e.ends_with("_nvenc") => cmd.args(["-c:v", e, "-preset", "p6", "-rc", "vbr", "-cq", "18", "-b:v", "0"]),
        e if e.ends_with("_qsv") => cmd.args(["-c:v", e, "-preset", "slow", "-global_quality", "18"]),
        e if e.ends_with("_amf") => cmd.args(["-c:v", e, "-quality", "quality", "-rc", "cqp", "-qp_i", "18", "-qp_p", "18"]),
        e if vaapi => cmd.args(["-c:v", e, "-qp", "18"]),
        other => cmd.args(["-c:v", other]),
    };
    let mut filters = Vec::new();
    if tagged {
        // The D1 fix: convert with the source matrix and range, and say so in the stream.
        filters.push("scale=out_color_matrix=bt709:out_range=tv".to_string());
        cmd.args(["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709", "-color_range", "tv"]);
    }
    if vaapi {
        filters.push("format=nv12,hwupload".to_string());
    } else {
        cmd.args(["-pix_fmt", "yuv420p"]);
    }
    if !filters.is_empty() {
        cmd.args(["-vf", &filters.join(",")]);
    }
    cmd.args(["-map_metadata", "-1", "-movflags", "+faststart", "-f", "mp4"]).arg(out);
    cmd.stdin(Stdio::piped());
    cmd
}

fn frame_bytes(width: usize, height: usize, pix_fmt: &str) -> usize {
    match pix_fmt {
        "bgr24" => width * height * 3,
        "yuv420p" => width * height * 3 / 2,
        _ => unreachable!(),
    }
}

fn wait(mut child: Child, what: &str) -> Result<()> {
    let status = child.wait()?;
    ensure!(status.success(), "{what} exited with {status}");
    Ok(())
}

/// Reads every frame from a decoder and returns how many arrived.
fn drain(mut cmd: Command, bytes: usize, mut each: impl FnMut(&mut [u8])) -> Result<usize> {
    let mut child = cmd.spawn()?;
    let mut out = BufReader::with_capacity(1 << 20, child.stdout.take().unwrap());
    let mut frame = vec![0u8; bytes];
    let mut n = 0;
    loop {
        match out.read_exact(&mut frame) {
            Ok(()) => {
                each(&mut frame);
                n += 1;
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
    }
    wait(child, "decoder")?;
    Ok(n)
}

/// A few decoded frames kept in memory and cycled, so encoder runs measure the encoder and the
/// pipe, not the decoder.
fn sample_frames(path: &Path, clip: &Clip, pix_fmt: &str, keep: usize) -> Result<Vec<Vec<u8>>> {
    let bytes = frame_bytes(clip.width, clip.height, pix_fmt);
    let mut frames = Vec::new();
    drain(decoder_limited(path, None, None, pix_fmt, Some(keep)), bytes, |f| frames.push(f.to_vec()))?;
    ensure!(frames.len() == keep, "decoded {} sample frames", frames.len());
    Ok(frames)
}

fn feed(mut cmd: Command, frames: &[Vec<u8>], count: usize) -> Result<()> {
    let mut child = cmd.spawn()?;
    {
        let mut stdin = BufWriter::with_capacity(1 << 20, child.stdin.take().unwrap());
        for i in 0..count {
            stdin.write_all(&frames[i % frames.len()])?;
        }
        stdin.flush()?;
    }
    wait(child, "encoder")
}

/// Stand-in for masking: fill a box per frame, touching memory the way the renderer would.
fn mask(frame: &mut [u8], width: usize, height: usize) {
    let (bw, bh) = (width / 6, height / 5);
    let (x0, y0) = (width / 3, height / 4);
    for y in y0..(y0 + bh).min(height) {
        let row = &mut frame[(y * width + x0) * 3..(y * width + x0 + bw) * 3];
        row.fill(0);
    }
}

fn full_pass(src: &Path, out: &Path, clip: &Clip, hw_decode: Option<&str>, codec: &str, tagged: bool) -> Result<usize> {
    let bytes = frame_bytes(clip.width, clip.height, "bgr24");
    let mut dec = decoder(src, hw_decode, None, "bgr24").spawn()?;
    let mut enc = encoder(out, clip.width, clip.height, "bgr24", codec, tagged).spawn()?;
    let mut reader = BufReader::with_capacity(1 << 20, dec.stdout.take().unwrap());
    let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(4);
    let (back_tx, back_rx) = mpsc::channel::<Vec<u8>>();
    for _ in 0..6 {
        back_tx.send(vec![0u8; bytes])?;
    }
    let read = std::thread::spawn(move || -> Result<()> {
        while let Ok(mut frame) = back_rx.recv() {
            match reader.read_exact(&mut frame) {
                Ok(()) => tx.send(frame)?,
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    });
    let mut n = 0;
    {
        let mut stdin = BufWriter::with_capacity(1 << 20, enc.stdin.take().unwrap());
        for mut frame in rx {
            mask(&mut frame, clip.width, clip.height);
            stdin.write_all(&frame)?;
            n += 1;
            let _ = back_tx.send(frame);
        }
        stdin.flush()?;
    }
    drop(back_tx);
    read.join().unwrap()?;
    wait(dec, "decoder")?;
    wait(enc, "encoder")?;
    Ok(n)
}

fn sink() -> Result<()> {
    let mut stdin = std::io::stdin().lock();
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = stdin.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
    }
    eprintln!("sink read {total} bytes");
    Ok(())
}

fn bench(dir: &Path, rest: &[String]) -> Result<()> {
    let mut hw_decode: Option<String> = Some(if cfg!(target_os = "macos") { "videotoolbox".into() } else { "none".into() });
    let mut hw_encoders: Vec<String> = if cfg!(target_os = "macos") {
        vec!["h264_videotoolbox".into(), "hevc_videotoolbox".into()]
    } else {
        Vec::new()
    };
    let mut only = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--hw-decode" => {
                hw_decode = rest.get(i + 1).cloned();
                i += 1;
            }
            "--hw-encoders" => {
                hw_encoders = rest.get(i + 1).map(|v| v.split(',').filter(|s| !s.is_empty()).map(String::from).collect()).unwrap_or_default();
                i += 1;
            }
            other => only.push(other.to_string()),
        }
        i += 1;
    }
    let hw = hw_decode.as_deref().filter(|h| *h != "none");
    let work = dir.join("out");
    std::fs::create_dir_all(&work)?;
    let me = std::env::current_exe()?;
    for clip in &CLIPS {
        if !only.is_empty() && !only.iter().any(|o| o == clip.name) {
            continue;
        }
        let src = dir.join(format!("{}.mp4", clip.name));
        ensure!(src.exists(), "{} missing; run gen first", src.display());
        let bgr = frame_bytes(clip.width, clip.height, "bgr24");
        let yuv = frame_bytes(clip.width, clip.height, "yuv420p");
        let analysis = if clip.width >= clip.height {
            (960, 960 * clip.height / clip.width / 2 * 2)
        } else {
            (960 * clip.width / clip.height / 2 * 2, 960)
        };

        // Upper bound of the pipe itself: this process writing frames to a copy of itself.
        {
            let frames = vec![vec![7u8; bgr]];
            let mut cmd = Command::new(&me);
            cmd.arg("sink").stdin(Stdio::piped()).stderr(Stdio::null());
            let m = Measure::start();
            feed(cmd, &frames, FRAMES)?;
            m.report("raw pipe ceiling (bgr24 frames, no ffmpeg)", clip.name, FRAMES, bgr, serde_json::json!({}));
        }

        // Decoder alone, frames discarded inside ffmpeg: no pipe.
        for accel in [None, hw] {
            let mut cmd = ffmpeg();
            if let Some(a) = accel {
                cmd.args(["-hwaccel", a]);
            }
            cmd.arg("-i").arg(&src);
            cmd.args(["-vf", &format!("setpts=PTS-STARTPTS,fps=fps={FPS}/1:start_time=0"), "-pix_fmt", "bgr24", "-f", "null", "-"]);
            let m = Measure::start();
            ensure!(cmd.status()?.success());
            let label = match accel {
                Some(a) => format!("decode→bgr24 inside ffmpeg, {a}"),
                None => "decode→bgr24 inside ffmpeg, software".to_string(),
            };
            m.report(&label, clip.name, FRAMES, 0, serde_json::json!({}));
            if hw.is_none() {
                break;
            }
        }

        let mut decodes: Vec<(String, Option<&str>, Option<(usize, usize)>, &str)> = vec![
            ("decode→bgr24→pipe, software (1.12.1 pass 2 reader)".into(), None, None, "bgr24"),
            ("decode→yuv420p→pipe, software".into(), None, None, "yuv420p"),
            ("decode→960 analysis bgr24→pipe, software (pass 1)".into(), None, Some(analysis), "bgr24"),
        ];
        if let Some(a) = hw {
            decodes.push((format!("decode→bgr24→pipe, {a}"), hw, None, "bgr24"));
            decodes.push((format!("decode→960 analysis bgr24→pipe, {a} (pass 1)"), hw, Some(analysis), "bgr24"));
        }
        for (label, accel, scale, fmt) in decodes {
            let bytes = match scale {
                Some((w, h)) => frame_bytes(w, h, fmt),
                None => frame_bytes(clip.width, clip.height, fmt),
            };
            let m = Measure::start();
            let n = drain(decoder(&src, accel, scale, fmt), bytes, |_| {})?;
            ensure!(n == FRAMES, "{label}: {n} frames");
            m.report(&label, clip.name, n, bytes, serde_json::json!({"frame_bytes": bytes}));
        }

        let samples = sample_frames(&src, clip, "bgr24", 8)?;
        let samples_yuv = sample_frames(&src, clip, "yuv420p", 8)?;
        let mut encodes: Vec<(String, String, &str, bool)> = vec![
            ("pipe→encode libx264 medium crf18 from bgr24 (1.12.1 default)".into(), "libx264".into(), "bgr24", false),
            ("pipe→encode libx264 medium crf18 from bgr24, bt709 tagged (D1)".into(), "libx264".into(), "bgr24", true),
            ("pipe→encode libx264 medium crf18 from yuv420p".into(), "libx264".into(), "yuv420p", false),
        ];
        for e in &hw_encoders {
            encodes.push((format!("pipe→encode {e} from bgr24"), e.clone(), "bgr24", false));
        }
        for (index, (label, codec, fmt, tagged)) in encodes.iter().enumerate() {
            let out = work.join(format!("{}-enc{index}.mp4", clip.name));
            let frames = if *fmt == "bgr24" { &samples } else { &samples_yuv };
            let m = Measure::start();
            feed(encoder(&out, clip.width, clip.height, fmt, codec, *tagged), frames, FRAMES)?;
            m.report(label, clip.name, FRAMES, if *fmt == "bgr24" { bgr } else { yuv }, serde_json::json!({}));
        }

        // The same encoder fed by ffmpeg itself from the file, with the same bgr24 round trip
        // inside the filter graph: what pass 2 would cost with no pipe at all.
        {
            let out = work.join(format!("{}-inproc.mp4", clip.name));
            let mut cmd = ffmpeg();
            cmd.arg("-i").arg(&src);
            cmd.args(["-vf", "format=bgr24,format=yuv420p", "-c:v", "libx264", "-preset", "medium", "-crf", "18", "-f", "mp4"]).arg(&out);
            let m = Measure::start();
            ensure!(cmd.status()?.success());
            m.report("decode→bgr24→encode libx264 inside one ffmpeg (no pipe)", clip.name, FRAMES, 0, serde_json::json!({}));
        }

        let mut passes: Vec<(String, Option<&str>, String)> =
            vec![("pass 2: decode→pipe→mask→pipe→libx264 (1.12.1 shape)".into(), None, "libx264".into())];
        if let Some(e) = hw_encoders.first() {
            passes.push((format!("pass 2: decode→pipe→mask→pipe→{e}"), None, e.clone()));
            if let Some(a) = hw {
                passes.push((format!("pass 2: {a} decode→pipe→mask→pipe→{e}"), hw, e.clone()));
            }
        }
        for (index, (label, accel, codec)) in passes.iter().enumerate() {
            let out = work.join(format!("{}-pass2-{index}.mp4", clip.name));
            let m = Measure::start();
            let n = full_pass(&src, &out, clip, *accel, codec, true)?;
            ensure!(n == FRAMES, "{label}: {n} frames");
            m.report(label, clip.name, n, bgr * 2, serde_json::json!({"pipe_bytes_counted": "both directions"}));
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("gen") => generate(&PathBuf::from(args.get(2).context("dir")?)),
        Some("bench") => bench(&PathBuf::from(args.get(2).context("dir")?), &args[3..]),
        Some("sink") => sink(),
        _ => bail!("usage: ffmpeg-pipe-spike gen <dir> | bench <dir> [--hw-decode X] [--hw-encoders a,b] [clips] | sink"),
    }
}
