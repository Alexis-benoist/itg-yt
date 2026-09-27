//! itg-yt: turns a YouTube video into a complete ITGmania song folder.
//!
//! 1. downloads audio, video (≤1080p) and thumbnail with yt-dlp, in parallel, into a
//!    cache (`~/.cache/itg-charter/youtube/<id>/`) so that reruns do not download again;
//! 2. encodes, in parallel with ffmpeg and into the same cache: the audio to OGG Vorbis
//!    q5 (≈160 kb/s, what ITG packs use), the thumbnail to banner / background / jacket
//!    images, and the video to silent H.264 (background movie, low CPU priority);
//! 3. as soon as the OGG and the images are ready, itg-charter generates the charts
//!    (called as a library) while the video is still encoding;
//! 4. itg-charter then adds the background movie.
//!
//! External tools: yt-dlp and ffmpeg (overridable with `ITG_YT_DLP` and `ITG_FFMPEG`).

use anyhow::{Context, Result, bail};
use clap::Parser;
use itg_charter::difficulty::parse_list;
use itg_charter::song::{self, Charts, SongOptions, VisualFiles};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(
    version,
    about = "Creates an ITGmania song folder (charts, audio, visuals, background video) from a YouTube URL"
)]
struct Args {
    /// YouTube URLs: videos, or playlists (a video URL with `&list=` stays one video).
    #[arg(required_unless_present = "batch_file")]
    urls: Vec<String>,
    /// File with one URL per line ("-" = stdin; blank lines and "#" comments ignored).
    #[arg(short = 'a', long)]
    batch_file: Option<PathBuf>,
    /// Songs downloaded and encoded at the same time. Charts are generated one song at a
    /// time (Demucs uses the GPU) while the next songs are being prepared.
    #[arg(short, long, default_value_t = 2)]
    jobs: usize,
    /// How many times a failed yt-dlp call is retried (pause of 2 s, 4 s, 8 s…).
    #[arg(long, default_value_t = 3)]
    yt_retries: u32,
    /// Named set of meters (used when neither --meters nor --difficulties is given),
    /// as in itg-charter: beginner = 2,3,4,5; full = 2,4,6,8,10.
    #[arg(short, long, value_enum, default_value_t = song::Profile::default())]
    profile: song::Profile,
    /// Meters to generate on the ITGmania scale (1-10), e.g. "2-5" or "1,3,6" (at most 5).
    #[arg(short, long, conflicts_with = "difficulties")]
    meters: Option<String>,
    /// Instead of meters: difficulty slots with the typical density of human charts,
    /// comma separated: beginner,easy,medium,hard,challenge or "all".
    #[arg(short, long)]
    difficulties: Option<String>,
    /// Random seed of the charts (same seed + same video = same charts).
    #[arg(short, long, default_value_t = 0)]
    seed: u64,
    /// Output folder (default: ~/ITG-YouTube, linked into the game's Songs folder, see
    /// README). The song folder is created inside.
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Skip the background video (it is included by default).
    #[arg(long)]
    no_video: bool,
    /// Skip Demucs source separation in itg-charter.
    #[arg(long)]
    no_stems: bool,
    /// Torch device for Demucs (cuda or cpu).
    #[arg(long)]
    device: Option<String>,
    /// Song title (default: from the YouTube metadata; only with a single video).
    #[arg(long)]
    title: Option<String>,
    /// Artist (default: from the YouTube metadata; only with a single video).
    #[arg(long)]
    artist: Option<String>,
    /// Maximum height of the background video in pixels (never upscaled). Lower is
    /// faster to encode and smaller (e.g. 720 or 480).
    #[arg(long, default_value_t = 1080)]
    video_height: u32,
    /// x264 preset of the background video: ultrafast, veryfast, fast, medium, slow...
    /// Faster presets encode quicker for a slightly bigger file.
    #[arg(long, default_value = "medium")]
    video_preset: String,
    /// H.264 quality of the background video (lower = better and bigger).
    #[arg(long, default_value_t = 26)]
    video_crf: u32,
    /// Download and encode cache folder (default: ~/.cache/itg-charter/youtube).
    #[arg(long)]
    cache: Option<PathBuf>,
}

/// The fields of `yt-dlp -j` we use (one JSON object per video).
#[derive(Deserialize, Debug, Default, Clone)]
struct Info {
    id: String,
    title: String,
    /// Canonical URL of the video (playlist URLs are resolved to their videos).
    webpage_url: Option<String>,
    /// Music metadata, filled by YouTube for many music videos.
    track: Option<String>,
    artists: Option<Vec<String>>,
    artist: Option<String>,
    creator: Option<String>,
    uploader: Option<String>,
    channel: Option<String>,
}

impl Info {
    fn url(&self) -> String {
        self.webpage_url
            .clone()
            .unwrap_or_else(|| format!("https://www.youtube.com/watch?v={}", self.id))
    }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
}

fn in_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

fn yt_dlp() -> PathBuf {
    if let Some(p) = std::env::var_os("ITG_YT_DLP") {
        return PathBuf::from(p);
    }
    let local = home().join(".local/bin/yt-dlp");
    if local.exists() {
        local
    } else {
        PathBuf::from("yt-dlp")
    }
}

fn ffmpeg() -> PathBuf {
    std::env::var_os("ITG_FFMPEG").map_or_else(|| PathBuf::from("ffmpeg"), PathBuf::from)
}

/// A running child process with a label for error messages.
struct Job {
    what: String,
    child: Child,
}

impl Job {
    fn spawn(what: &str, cmd: &mut Command) -> Result<Job> {
        let child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("{what}: cannot start {:?}", cmd.get_program()))?;
        Ok(Job {
            what: what.to_string(),
            child,
        })
    }

    fn wait(self) -> Result<Output> {
        let out = self.child.wait_with_output()?;
        if !out.status.success() {
            bail!(
                "{} failed:\n{}",
                self.what,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(out)
    }
}

/// An encode into the cache: written under a temporary name and renamed when done,
/// so that an interrupted run never leaves a truncated file that would be reused.
struct Encode {
    job: Option<Job>,
    tmp: PathBuf,
    path: PathBuf,
}

impl Encode {
    /// Starts `cmd` (with the temporary output path appended) unless `path` exists.
    fn start(what: &str, mut cmd: Command, path: PathBuf) -> Result<Encode> {
        let ext = path.extension().unwrap_or_default().to_string_lossy();
        let tmp = path.with_extension(format!("partial.{ext}"));
        let job = if path.exists() {
            None
        } else {
            Some(Job::spawn(what, cmd.arg(&tmp))?)
        };
        Ok(Encode { job, tmp, path })
    }

    fn wait(self) -> Result<PathBuf> {
        if let Some(job) = self.job {
            job.wait()?;
            std::fs::rename(&self.tmp, &self.path)?;
        }
        Ok(self.path)
    }
}

/// yt-dlp command with the options shared by every call.
fn yt_dlp_base() -> Command {
    let mut c = Command::new(yt_dlp());
    c.args(["--no-playlist", "--no-progress", "--quiet", "--no-warnings"]);
    // yt-dlp's own retries, for network errors inside one call.
    c.args([
        "--retries",
        "10",
        "--fragment-retries",
        "10",
        "--extractor-retries",
        "3",
    ]);
    // YouTube needs a JavaScript runtime; yt-dlp only enables deno by default.
    if !in_path("deno") && in_path("node") {
        c.args(["--js-runtimes", "node"]);
    }
    c
}

/// yt-dlp command for one video.
fn yt_dlp_cmd(url: &str) -> Command {
    let mut c = yt_dlp_base();
    c.arg(url);
    c
}

/// Pause before retry number `attempt` (1-based): 2 s, 4 s, 8 s…
/// (`ITG_YT_RETRY_PAUSE_MS` sets the first pause, for tests).
fn retry_pause(attempt: u32) -> Duration {
    let first = std::env::var("ITG_YT_RETRY_PAUSE_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000u64);
    Duration::from_millis(first << (attempt - 1).min(6))
}

/// Runs a command built by `make`, retrying up to `retries` times when it fails
/// (yt-dlp sometimes crashes or fails to extract a video, then works on a new try).
fn run_with_retries(
    what: &str,
    retries: u32,
    make: impl Fn() -> Command,
    log: impl Fn(&str),
) -> Result<Output> {
    let mut attempt = 0;
    loop {
        match Job::spawn(what, &mut make()).and_then(Job::wait) {
            Ok(out) => return Ok(out),
            Err(e) if attempt < retries => {
                attempt += 1;
                let pause = retry_pause(attempt);
                let reason = e.to_string();
                log(&format!(
                    "{what} failed, retry {attempt}/{retries} in {:.0}s ({})",
                    pause.as_secs_f64(),
                    reason.lines().last().unwrap_or_default()
                ));
                std::thread::sleep(pause);
            }
            Err(e) => return Err(e),
        }
    }
}

/// First non-partial file of the cache folder whose name starts with `stem.`.
fn cached(dir: &Path, stem: &str) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy();
            name.starts_with(&format!("{stem}."))
                && !name.ends_with(".part")
                && !name.ends_with(".ytdl")
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

/// Whether the content of a bracket group is YouTube decoration ("Official Video",
/// "Lyrics", "4K", "M/V"...), as opposed to part of the song name ("feat. X", "Remix").
fn is_noise(inner: &str) -> bool {
    const PREFIXES: [&str; 8] = [
        "official", "video", "lyric", "audio", "visuali", "clip", "remaster", "colo",
    ];
    const WORDS: [&str; 5] = ["mv", "m/v", "hd", "4k", "hq"];
    let lower = inner.to_lowercase();
    lower
        .split(|c: char| !c.is_alphanumeric() && c != '/')
        .filter(|w| !w.is_empty())
        .any(|w| WORDS.contains(&w) || PREFIXES.iter().any(|p| w.starts_with(p)))
}

/// Removes decorations like "(Official Video)" or "[Lyrics]" from a video title.
fn clean_title(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find(['(', '[', '【']) {
        let open = rest[start..].chars().next().unwrap();
        let close = match open {
            '(' => ')',
            '[' => ']',
            _ => '】',
        };
        let Some(len) = rest[start..].find(close) else {
            break;
        };
        let end = start + len + close.len_utf8();
        out.push_str(&rest[..start]);
        if !is_noise(&rest[start + open.len_utf8()..start + len]) {
            out.push_str(&rest[start..end]);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(['-', '|', ' '])
        .to_string()
}

/// (title, artist) from the YouTube metadata.
fn song_names(info: &Info) -> (String, String) {
    let artists = info
        .artists
        .as_ref()
        .filter(|a| !a.is_empty())
        .map(|a| a.join(", "));
    let uploader = artists
        .or(info.artist.clone())
        .or(info.creator.clone())
        .or(info.channel.clone())
        .or(info.uploader.clone())
        .unwrap_or_default()
        .trim_end_matches(" - Topic")
        .trim_end_matches("VEVO")
        .trim()
        .to_string();
    if let Some(track) = info.track.as_ref().filter(|t| !t.trim().is_empty()) {
        return (clean_title(track), uploader);
    }
    for sep in [" - ", " – ", " — ", " | "] {
        if let Some((artist, title)) = info.title.split_once(sep) {
            return (clean_title(title), clean_title(artist));
        }
    }
    (clean_title(&info.title), uploader)
}

/// A file name derived from the title (the audio and movie keep it in the song
/// folder). itg-charter makes it safe for simfile tags; here we only avoid path
/// separators and characters that file systems reject.
fn file_stem(title: &str) -> String {
    let s: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            c => c,
        })
        .collect();
    let s = s.trim().trim_start_matches('.').to_string();
    if s.is_empty() { "song".into() } else { s }
}

fn ffmpeg_cmd() -> Command {
    let mut c = Command::new(ffmpeg());
    c.args(["-y", "-nostdin", "-hide_banner", "-loglevel", "error"]);
    c
}

/// ffmpeg at low CPU priority (for the long video encode, so that the audio and the
/// charts, which the rest of the pipeline waits for, get the cores first).
fn low_priority_ffmpeg_cmd() -> Command {
    if !in_path("nice") {
        return ffmpeg_cmd();
    }
    let mut c = Command::new("nice");
    c.args(["-n", "15"]).arg(ffmpeg());
    c.args(["-y", "-nostdin", "-hide_banner", "-loglevel", "error"]);
    c
}

/// Center-cropped still image of `w`×`h` from the thumbnail.
fn image_encode(thumb: &Path, out: PathBuf, w: u32, h: u32) -> Result<Encode> {
    let mut cmd = ffmpeg_cmd();
    cmd.arg("-i").arg(thumb).args([
        "-vf",
        &format!("scale={w}:{h}:force_original_aspect_ratio=increase:flags=lanczos,crop={w}:{h}"),
        "-frames:v",
        "1",
        "-fflags",
        "+bitexact",
    ]);
    Encode::start(&format!("image {w}x{h}"), cmd, out)
}

/// Everything shared by the songs of one run.
struct Ctx {
    args: Args,
    cache_root: PathBuf,
    output: PathBuf,
    t0: Instant,
    /// Charts to generate for every song.
    charts: Charts,
    /// Held while the charts are generated: Demucs on a 4 GB GPU, one song at a time.
    charting: std::sync::Mutex<()>,
}

impl Ctx {
    fn log(&self, song: &str, msg: &str) {
        eprintln!("[{:>6.1}s] {song}: {msg}", self.t0.elapsed().as_secs_f64());
    }
}

/// All URLs of the run: arguments, then the batch file (if any), in order.
fn collect_urls(args: &Args) -> Result<Vec<String>> {
    let mut urls = args.urls.clone();
    if let Some(file) = &args.batch_file {
        let text = if file.as_os_str() == "-" {
            std::io::read_to_string(std::io::stdin())?
        } else {
            std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?
        };
        urls.extend(
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(String::from),
        );
    }
    anyhow::ensure!(!urls.is_empty(), "no URL given");
    Ok(urls)
}

/// Metadata of every video with a single yt-dlp call (`-j`: one JSON line per video;
/// playlists are expanded, a `watch?v=…&list=…` URL stays one video). Unavailable
/// videos are reported and skipped; duplicates are removed, order is kept. When some
/// URLs fail, the call is repeated (up to `retries` times) and the results merged.
fn fetch_infos(
    urls: &[String],
    retries: u32,
    log: impl Fn(&str),
) -> Result<(Vec<Info>, Vec<String>)> {
    let mut infos: Vec<Info> = Vec::new();
    let mut attempt = 0;
    loop {
        let mut cmd = yt_dlp_base();
        cmd.args(["-j", "--ignore-errors"]).args(urls);
        let out = cmd
            .stdin(Stdio::null())
            .output()
            .context("cannot start yt-dlp")?;
        for line in String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
        {
            let info: Info = serde_json::from_str(line).context("parsing yt-dlp metadata")?;
            if !infos.iter().any(|i| i.id == info.id) {
                infos.push(info);
            }
        }
        let errors: Vec<String> = String::from_utf8_lossy(&out.stderr)
            .lines()
            .filter(|l| l.starts_with("ERROR"))
            .map(String::from)
            .collect();
        if errors.is_empty() || attempt >= retries {
            return Ok((infos, errors));
        }
        attempt += 1;
        let pause = retry_pause(attempt);
        log(&format!(
            "{} URL(s) failed, retry {attempt}/{retries} in {:.0}s",
            errors.len(),
            pause.as_secs_f64()
        ));
        std::thread::sleep(pause);
    }
}

/// Downloads, encodes and charts one song; returns its `.sm`.
fn process(ctx: &Ctx, info: &Info, title: &str, artist: &str) -> Result<PathBuf> {
    let args = &ctx.args;
    let url = info.url();
    let log = |msg: &str| ctx.log(title, msg);
    let cache = ctx.cache_root.join(file_stem(&info.id));
    std::fs::create_dir_all(&cache)?;

    // Downloads, in parallel.
    let template = |stem: &str| {
        cache
            .join(format!("{stem}.%(ext)s"))
            .to_string_lossy()
            .into_owned()
    };
    let video_format = format!(
        "bestvideo[height<={h}][vcodec^=avc1]/bestvideo[height<={h}]/best[height<={h}]",
        h = args.video_height
    );
    let mut downloads: Vec<(&str, Vec<String>)> = Vec::new();
    if cached(&cache, "audio").is_none() {
        downloads.push((
            "audio download",
            vec![
                "-f".into(),
                "bestaudio/best".into(),
                "-o".into(),
                template("audio"),
            ],
        ));
    }
    if !args.no_video && cached(&cache, "video").is_none() {
        downloads.push((
            "video download",
            vec!["-f".into(), video_format, "-o".into(), template("video")],
        ));
    }
    if cached(&cache, "thumb").is_none() {
        downloads.push((
            "thumbnail download",
            vec![
                "--skip-download".into(),
                "--write-thumbnail".into(),
                "--convert-thumbnails".into(),
                "jpg".into(),
                "-o".into(),
                template("thumb"),
            ],
        ));
    }
    if !downloads.is_empty() {
        log(&format!(
            "downloading ({} files in parallel)",
            downloads.len()
        ));
    }
    std::thread::scope(|s| {
        let handles: Vec<_> = downloads
            .iter()
            .map(|(what, extra)| {
                let url = &url;
                s.spawn(move || {
                    run_with_retries(
                        what,
                        args.yt_retries,
                        || {
                            let mut c = yt_dlp_cmd(url);
                            c.args(extra);
                            c
                        },
                        log,
                    )
                })
            })
            .collect();
        handles
            .into_iter()
            .try_for_each(|h| h.join().expect("download thread").map(|_| ()))
    })?;
    let audio_src = cached(&cache, "audio").context("audio was not downloaded")?;
    let thumb = cached(&cache, "thumb").context("thumbnail was not downloaded")?;
    let video_src = if args.no_video {
        None
    } else {
        Some(cached(&cache, "video").context("video was not downloaded")?)
    };

    // Encodes into the cache, in parallel. Files already encoded are reused (the video
    // per height / preset / quality).
    let stem = file_stem(title);
    let encoded = cache.join("encoded");
    std::fs::create_dir_all(&encoded)?;
    log("encoding audio, images and video in parallel");
    let mut audio_cmd = ffmpeg_cmd();
    audio_cmd.arg("-i").arg(&audio_src).args([
        "-vn",
        "-map_metadata",
        "-1",
        "-ac",
        "2",
        "-ar",
        "44100",
        "-c:a",
        "libvorbis",
        "-q:a",
        "5",
        "-fflags",
        "+bitexact",
        "-flags:a",
        "+bitexact",
    ]);
    let audio = Encode::start(
        "audio encode",
        audio_cmd,
        encoded.join(format!("{stem}.ogg")),
    )?;
    let images = [
        image_encode(&thumb, encoded.join("bn.png"), 418, 164)?,
        image_encode(&thumb, encoded.join("bg.png"), 1920, 1080)?,
        image_encode(&thumb, encoded.join("jacket.png"), 512, 512)?,
    ];
    let video = match &video_src {
        Some(src) => {
            let dir = encoded.join(format!(
                "video-{}p-{}-crf{}",
                args.video_height, args.video_preset, args.video_crf
            ));
            std::fs::create_dir_all(&dir)?;
            let mut cmd = low_priority_ffmpeg_cmd();
            cmd.arg("-i").arg(src).args([
                "-an",
                "-map_metadata",
                "-1",
                "-vf",
                &format!(
                    "scale=-2:'min({},ih)':flags=lanczos,fps=30",
                    args.video_height
                ),
                "-c:v",
                "libx264",
                "-preset",
                &args.video_preset,
                "-crf",
                &args.video_crf.to_string(),
                "-pix_fmt",
                "yuv420p",
                "-movflags",
                "+faststart",
            ]);
            Some(Encode::start(
                "video encode",
                cmd,
                dir.join(format!("{stem}-bg.mp4")),
            )?)
        }
        None => None,
    };
    let ogg = audio.wait()?;
    let [banner, background, jacket] = images.map(Encode::wait);
    let (banner, background, jacket) = (banner?, background?, jacket?);

    // Song folder and charts (itg-charter library, one song at a time: Demucs uses the
    // GPU), while the video keeps encoding.
    let opts = SongOptions {
        charts: ctx.charts.clone(),
        seed: args.seed,
        output: ctx.output.clone(),
        title: Some(title.to_string()),
        artist: Some(artist.to_string()),
        stems: !args.no_stems,
        device: args.device.clone(),
        visuals: VisualFiles {
            banner: Some(banner),
            background: Some(background),
            jacket: Some(jacket),
            bg_video: None,
        },
        ..SongOptions::default()
    };
    let sm = {
        let _gpu = ctx.charting.lock().unwrap_or_else(|e| e.into_inner());
        log("generating charts");
        song::create_song(&ogg, &opts)?
    };

    if let Some(video) = video {
        log("charts ready, waiting for the video encode");
        let movie = video.wait()?;
        song::decorate(
            &sm,
            &VisualFiles {
                bg_video: Some(movie),
                ..VisualFiles::default()
            },
        )?;
    }
    log("done");
    Ok(sm)
}

/// Charts requested on the command line, checked before any download.
fn charts_of(args: &Args) -> Result<Charts> {
    let charts = match (&args.difficulties, &args.meters) {
        (Some(d), _) => Charts::Slots(parse_list(d)?),
        (None, Some(m)) => Charts::Meters(song::parse_meters(m)?),
        (None, None) => Charts::Meters(args.profile.meters()),
    };
    if let Charts::Meters(m) = &charts {
        itg_charter::chart::assign_slots(&itg_charter::model::Model::embedded()?, m)?;
    }
    Ok(charts)
}

fn main() -> Result<()> {
    let args = Args::parse();
    let urls = collect_urls(&args)?;
    let charts = charts_of(&args)?;
    let ctx = Ctx {
        charts,
        cache_root: args
            .cache
            .clone()
            .unwrap_or_else(|| home().join(".cache/itg-charter/youtube")),
        output: args
            .output
            .clone()
            .unwrap_or_else(|| home().join("ITG-YouTube")),
        t0: Instant::now(),
        charting: std::sync::Mutex::new(()),
        args,
    };

    ctx.log(
        "yt-dlp",
        &format!("reading metadata of {} URL(s)", urls.len()),
    );
    let (infos, errors) = fetch_infos(&urls, ctx.args.yt_retries, |m| ctx.log("yt-dlp", m))?;
    for e in &errors {
        ctx.log("yt-dlp", e);
    }
    anyhow::ensure!(!infos.is_empty(), "no video found");
    if infos.len() > 1 && (ctx.args.title.is_some() || ctx.args.artist.is_some()) {
        bail!(
            "--title / --artist only make sense with a single video ({} found)",
            infos.len()
        );
    }
    let songs: Vec<(Info, String, String)> = infos
        .into_iter()
        .map(|info| {
            let (t, a) = song_names(&info);
            let title = ctx.args.title.clone().unwrap_or(t);
            let artist = ctx.args.artist.clone().unwrap_or(a);
            (info, title, artist)
        })
        .collect();
    for (i, (info, title, artist)) in songs.iter().enumerate() {
        ctx.log(
            "yt-dlp",
            &format!(
                "{}/{}: \"{title}\" by \"{artist}\" ({})",
                i + 1,
                songs.len(),
                info.id
            ),
        );
    }

    // Worker pool: `jobs` songs prepared at the same time, charts one at a time.
    let next = std::sync::Mutex::new(0usize);
    let results: std::sync::Mutex<Vec<(usize, Result<PathBuf>)>> =
        std::sync::Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..ctx.args.jobs.clamp(1, songs.len()) {
            s.spawn(|| {
                loop {
                    let i = {
                        let mut n = next.lock().unwrap();
                        *n += 1;
                        *n - 1
                    };
                    let Some((info, title, artist)) = songs.get(i) else {
                        break;
                    };
                    let r = process(&ctx, info, title, artist);
                    if let Err(e) = &r {
                        ctx.log(title, &format!("FAILED: {e:#}"));
                    }
                    results.lock().unwrap().push((i, r));
                }
            });
        }
    });

    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|r| r.0);
    let failed = results.iter().filter(|r| r.1.is_err()).count() + errors.len();
    eprintln!(
        "\n{} song(s) ready, {failed} failed ({:.0}s):",
        results.len() - results.iter().filter(|r| r.1.is_err()).count(),
        ctx.t0.elapsed().as_secs_f64()
    );
    for (i, r) in &results {
        match r {
            Ok(sm) => {
                eprintln!("  ok      {}", songs[*i].1);
                println!("{}", sm.display());
            }
            Err(e) => eprintln!("  FAILED  {}: {e}", songs[*i].1),
        }
    }
    for e in &errors {
        eprintln!("  FAILED  {e}");
    }
    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(title: &str) -> Info {
        Info {
            id: "x".into(),
            title: title.into(),
            ..Info::default()
        }
    }

    #[test]
    fn cleans_youtube_titles() {
        let (t, a) = song_names(&info("Daft Punk - One More Time (Official Video) [4K]"));
        assert_eq!((t.as_str(), a.as_str()), ("One More Time", "Daft Punk"));
        assert_eq!(
            clean_title("Song (feat. Someone) [Lyrics]"),
            "Song (feat. Someone)"
        );
        assert_eq!(
            clean_title("Song (Remix) (Official Music Video)"),
            "Song (Remix)"
        );
        assert_eq!(clean_title("TWICE \"FANCY\" M/V"), "TWICE \"FANCY\" M/V");
        let (t, a) = song_names(&Info {
            track: Some("Levitating".into()),
            artist: Some("Dua Lipa".into()),
            ..info("Dua Lipa - Levitating (Official Music Video)")
        });
        assert_eq!((t.as_str(), a.as_str()), ("Levitating", "Dua Lipa"));
        let (t, a) = song_names(&Info {
            uploader: Some("Band - Topic".into()),
            ..info("Tune")
        });
        assert_eq!((t.as_str(), a.as_str()), ("Tune", "Band"));
    }

    #[test]
    fn file_stems_are_safe() {
        assert_eq!(file_stem("AC/DC: Back In Black?"), "AC_DC_ Back In Black_");
        assert_eq!(file_stem("..."), "song");
        assert_eq!(
            file_stem("Yeah! ft. Lil Jon, Ludacris"),
            "Yeah! ft. Lil Jon, Ludacris"
        );
    }
}
