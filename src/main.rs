//! itg-yt: turns a YouTube video into a complete ITGmania song folder.
//!
//! 1. downloads audio, video (≤1080p) and thumbnail with yt-dlp, in parallel, into a
//!    cache (`~/.cache/itg-charter/youtube/<id>/`) so that reruns do not download again;
//! 2. encodes, in parallel with ffmpeg: the audio to OGG Vorbis q5 (≈160 kb/s, what
//!    ITG packs use), the video to H.264 ≤1080p without sound (background movie), and
//!    the thumbnail to banner / background / jacket images;
//! 3. runs `itg-charter gen` on the final OGG (the file the game plays) while the
//!    video is still encoding;
//! 4. declares the images and the background movie in the generated `.sm`.
//!
//! Tools can be overridden with `ITG_YT_DLP`, `ITG_FFMPEG` and `ITG_CHARTER_BIN`.
//! itg-charter (https://github.com/Alexis-benoist/itg-charter) is used as a program:
//! this crate does not link it, it only reads the `.sm` it writes.

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::Instant;

#[derive(Parser)]
#[command(
    version,
    about = "Creates an ITGmania song folder (charts, audio, visuals, background video) from a YouTube URL"
)]
struct Args {
    /// YouTube video URL.
    url: String,
    /// Difficulties, comma separated: beginner,easy,medium,hard,challenge or "all".
    #[arg(short, long, default_value = "all")]
    difficulties: String,
    /// Random seed of the charts (same seed + same video = same charts).
    #[arg(short, long, default_value_t = 0)]
    seed: u64,
    /// Output folder (default: ~/.itgmania/Songs/YouTube). The song folder is created inside.
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
    /// Song title (default: from the YouTube metadata).
    #[arg(long)]
    title: Option<String>,
    /// Artist (default: from the YouTube metadata).
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
    /// Download cache folder (default: ~/.cache/itg-charter/youtube).
    #[arg(long)]
    cache: Option<PathBuf>,
}

/// The fields of `yt-dlp -J` we use.
#[derive(Deserialize, Debug, Default)]
struct Info {
    id: String,
    title: String,
    track: Option<String>,
    artist: Option<String>,
    creator: Option<String>,
    uploader: Option<String>,
    channel: Option<String>,
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
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

/// The itg-charter binary: `$ITG_CHARTER_BIN`, else `itg-charter` in the PATH, else a
/// release build in `~/itg-charter`.
fn itg_charter() -> PathBuf {
    if let Some(p) = std::env::var_os("ITG_CHARTER_BIN") {
        return PathBuf::from(p);
    }
    if in_path("itg-charter") {
        return PathBuf::from("itg-charter");
    }
    home().join("itg-charter/target/release/itg-charter")
}

/// Removes characters that would break the `#TAG:VALUE;` syntax (same rule as
/// itg-charter, which names the song folder after the sanitized title).
fn sanitize(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ';' | '#' | '\\' | '\n' | '\r'))
        .map(|c| if c == ':' { '-' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Value of the first `#KEY:VALUE;` tag of a simfile.
fn sm_tag<'a>(sm: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("#{key}:");
    let start = sm.find(&marker)? + marker.len();
    let end = sm[start..].find(';')?;
    Some(sm[start..start + end].trim())
}

fn in_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
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

/// yt-dlp command with the options shared by every call.
fn yt_dlp_cmd(url: &str) -> Command {
    let mut c = Command::new(yt_dlp());
    c.args(["--no-playlist", "--no-progress", "--quiet", "--no-warnings"]);
    // YouTube needs a JavaScript runtime; yt-dlp only enables deno by default.
    if !in_path("deno") && in_path("node") {
        c.args(["--js-runtimes", "node"]);
    }
    c.arg(url);
    c
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
    let uploader = info
        .artist
        .clone()
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

/// Name of the song folder, computed like `itg-charter gen` does from the title.
fn folder_name(title: &str) -> String {
    let f = sanitize(title).replace(['/', '?', '*', '"', '<', '>', '|'], "_");
    if f.is_empty() { "song".into() } else { f }
}

/// Replaces the value of `#KEY:...;`, or inserts the tag before the first chart.
fn set_tag(sm: &str, key: &str, value: &str) -> String {
    let marker = format!("#{key}:");
    if let Some(start) = sm.find(&marker) {
        let value_start = start + marker.len();
        if let Some(end) = sm[value_start..].find(';') {
            return format!("{}{value}{}", &sm[..value_start], &sm[value_start + end..]);
        }
    }
    let at = sm
        .find("\n//---")
        .or_else(|| sm.find("#NOTES"))
        .unwrap_or(sm.len());
    format!(
        "{}\n#{key}:{value};{}",
        sm[..at].trim_end_matches('\n'),
        &sm[at..]
    )
}

/// Beat at which the audio time is 0 (where the background movie must start).
/// itg-charter writes one constant BPM: time(beat) = beat × 60 / BPM − OFFSET.
fn movie_start_beat(sm: &str) -> Result<f64> {
    let offset: f64 = sm_tag(sm, "OFFSET").context("no #OFFSET")?.parse()?;
    let bpms = sm_tag(sm, "BPMS").context("no #BPMS")?;
    anyhow::ensure!(!bpms.contains(','), "BPM changes are not supported: {bpms}");
    let (_, bpm) = bpms.split_once('=').context("malformed #BPMS")?;
    Ok(offset * bpm.trim().parse::<f64>()? / 60.0)
}

/// Adds banner, background, jacket and background movie to the `.sm` text.
fn add_visuals(text: &str, movie: Option<&str>) -> Result<String> {
    let mut sm = set_tag(text, "BANNER", "bn.png");
    sm = set_tag(&sm, "BACKGROUND", "bg.png");
    sm = set_tag(&sm, "JACKET", "jacket.png");
    if let Some(movie) = movie {
        let beat = movie_start_beat(text)?;
        sm = set_tag(
            &sm,
            "BGCHANGES",
            &format!("{beat:.3}={movie}=1.000=0=0=0=StretchNoLoop===="),
        );
    }
    Ok(sm)
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
fn image_job(thumb: &Path, out: &Path, w: u32, h: u32) -> Result<Job> {
    Job::spawn(
        &format!("image {}", out.display()),
        ffmpeg_cmd()
            .arg("-i")
            .arg(thumb)
            .args([
                "-vf",
                &format!(
                    "scale={w}:{h}:force_original_aspect_ratio=increase:flags=lanczos,crop={w}:{h}"
                ),
                "-frames:v",
                "1",
                "-fflags",
                "+bitexact",
            ])
            .arg(out),
    )
}

fn main() -> Result<()> {
    let args = Args::parse();
    let t0 = Instant::now();
    let step = |msg: &str| eprintln!("[{:>6.1}s] {msg}", t0.elapsed().as_secs_f64());
    let cache_root = args
        .cache
        .clone()
        .unwrap_or_else(|| home().join(".cache/itg-charter/youtube"));
    let output = args
        .output
        .clone()
        .unwrap_or_else(|| home().join(".itgmania/Songs/YouTube"));

    // Metadata.
    step("reading video metadata");
    let meta = Job::spawn("yt-dlp metadata", yt_dlp_cmd(&args.url).arg("-J"))?.wait()?;
    let info: Info = serde_json::from_slice(&meta.stdout).context("parsing yt-dlp metadata")?;
    let cache = cache_root.join(sanitize(&info.id).replace(['/', '\\'], "_"));
    std::fs::create_dir_all(&cache)?;
    let (auto_title, auto_artist) = song_names(&info);
    let title = args.title.clone().unwrap_or(auto_title);
    let artist = args.artist.clone().unwrap_or(auto_artist);
    step(&format!("\"{title}\" by \"{artist}\" ({})", info.id));

    // Downloads, in parallel.
    let template = |stem: &str| {
        cache
            .join(format!("{stem}.%(ext)s"))
            .to_string_lossy()
            .into_owned()
    };
    let mut downloads = Vec::new();
    if cached(&cache, "audio").is_none() {
        downloads.push(Job::spawn(
            "audio download",
            yt_dlp_cmd(&args.url).args(["-f", "bestaudio/best", "-o", &template("audio")]),
        )?);
    }
    if !args.no_video && cached(&cache, "video").is_none() {
        downloads.push(Job::spawn(
            "video download",
            yt_dlp_cmd(&args.url).args([
                "-f",
                &format!(
                    "bestvideo[height<={h}][vcodec^=avc1]/bestvideo[height<={h}]/best[height<={h}]",
                    h = args.video_height
                ),
                "-o",
                &template("video"),
            ]),
        )?);
    }
    if cached(&cache, "thumb").is_none() {
        downloads.push(Job::spawn(
            "thumbnail download",
            yt_dlp_cmd(&args.url).args([
                "--skip-download",
                "--write-thumbnail",
                "--convert-thumbnails",
                "jpg",
                "-o",
                &template("thumb"),
            ]),
        )?);
    }
    if !downloads.is_empty() {
        step(&format!(
            "downloading ({} files in parallel)",
            downloads.len()
        ));
    }
    for d in downloads {
        d.wait()?;
    }
    let audio_src = cached(&cache, "audio").context("audio was not downloaded")?;
    let thumb = cached(&cache, "thumb").context("thumbnail was not downloaded")?;
    let video_src = if args.no_video {
        None
    } else {
        Some(cached(&cache, "video").context("video was not downloaded")?)
    };

    // Encodes, in parallel.
    let folder = folder_name(&title);
    let dir = output.join(&folder);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let ogg = dir.join(format!("{folder}.ogg"));
    let movie_name = format!("{}-bg.mp4", folder.replace(['=', ','], "_"));
    step("encoding audio, video and images in parallel");
    let audio_job = Job::spawn(
        "audio encode",
        ffmpeg_cmd()
            .arg("-i")
            .arg(&audio_src)
            .args([
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
            ])
            .arg(&ogg),
    )?;
    let video_job = match &video_src {
        Some(src) => Some(Job::spawn(
            "video encode",
            low_priority_ffmpeg_cmd()
                .arg("-i")
                .arg(src)
                .args([
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
                ])
                .arg(dir.join(&movie_name)),
        )?),
        None => None,
    };
    let image_jobs = vec![
        image_job(&thumb, &dir.join("bn.png"), 418, 164)?,
        image_job(&thumb, &dir.join("bg.png"), 1920, 1080)?,
        image_job(&thumb, &dir.join("jacket.png"), 512, 512)?,
    ];

    // Charts, as soon as the OGG exists (the video keeps encoding meanwhile).
    audio_job.wait()?;
    step("audio ready, generating charts (video still encoding)");
    let mut charter = Command::new(itg_charter());
    charter
        .arg("gen")
        .arg(&ogg)
        .arg("-o")
        .arg(&output)
        .args(["-d", &args.difficulties, "-s", &args.seed.to_string()])
        .args(["--title", &title, "--artist", &artist]);
    if args.no_stems {
        charter.arg("--no-stems");
    }
    if let Some(d) = &args.device {
        charter.args(["--device", d]);
    }
    let gen_out = Job::spawn("itg-charter gen", &mut charter)?.wait()?;
    eprint!("{}", String::from_utf8_lossy(&gen_out.stderr));
    let sm_path = PathBuf::from(
        String::from_utf8_lossy(&gen_out.stdout)
            .trim()
            .lines()
            .last()
            .unwrap_or_default(),
    );
    anyhow::ensure!(sm_path.exists(), "itg-charter did not report a simfile");
    anyhow::ensure!(
        sm_path.parent() == Some(dir.as_path()),
        "itg-charter wrote {} instead of {}",
        sm_path.display(),
        dir.display()
    );

    for j in image_jobs {
        j.wait()?;
    }
    if let Some(j) = video_job {
        step("waiting for the video encode");
        j.wait()?;
    }
    let text = std::fs::read_to_string(&sm_path)?;
    std::fs::write(
        &sm_path,
        add_visuals(&text, video_src.as_ref().map(|_| movie_name.as_str()))?,
    )?;
    step("done");
    println!("{}", sm_path.display());
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
    fn visuals_are_declared_and_movie_starts_at_audio_zero() {
        let sm = "#TITLE:t;\n#BANNER:;\n#BACKGROUND:;\n#OFFSET:-0.250;\n#BPMS:0.000=120.000;\n#BGCHANGES:;\n\n//---------------dance-single - x----------------\n#NOTES:\n     dance-single:\n     x:\n     Easy:\n     1:\n     0,0,0,0,0:\n1000\n;\n";
        let out = add_visuals(sm, Some("t-bg.mp4")).unwrap();
        assert_eq!(sm_tag(&out, "BANNER"), Some("bn.png"));
        assert_eq!(sm_tag(&out, "BACKGROUND"), Some("bg.png"));
        assert_eq!(sm_tag(&out, "JACKET"), Some("jacket.png"));
        // offset -0.25 s at 120 BPM: audio time 0 is beat -0.5.
        assert_eq!(
            sm_tag(&out, "BGCHANGES"),
            Some("-0.500=t-bg.mp4=1.000=0=0=0=StretchNoLoop====")
        );
        // The JACKET tag goes before the charts, which are untouched.
        assert!(out.find("#JACKET").unwrap() < out.find("#NOTES").unwrap());
        assert!(out.ends_with(
            "#NOTES:\n     dance-single:\n     x:\n     Easy:\n     1:\n     0,0,0,0,0:\n1000\n;\n"
        ));
    }
}
