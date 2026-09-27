//! Offline end-to-end tests: a fake yt-dlp serves generated media, ffmpeg runs for real
//! (through a logging wrapper). itg-charter is a fake by default; the last test uses
//! the real one when it is installed. Skipped when ffmpeg/ffprobe are missing.

use std::path::{Path, PathBuf};
use std::process::Command;

fn has(tool: &str) -> bool {
    Command::new(tool)
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn ffmpeg(args: &[&str]) {
    let st = Command::new("ffmpeg")
        .args(["-y", "-nostdin", "-loglevel", "error"])
        .args(args)
        .status()
        .unwrap();
    assert!(st.success(), "ffmpeg {args:?}");
}

fn probe(file: &Path, entries: &str) -> String {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            entries,
            "-of",
            "default=nw=1",
        ])
        .arg(file)
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap()
}

fn write_script(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
    let mut perm = std::fs::metadata(path).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
    std::fs::set_permissions(path, perm).unwrap();
}

/// The real itg-charter, if built next to this repository or given by ITG_CHARTER_REAL.
fn real_itg_charter() -> Option<PathBuf> {
    std::env::var_os("ITG_CHARTER_REAL")
        .map(PathBuf::from)
        .or_else(|| {
            let home = PathBuf::from(std::env::var_os("HOME")?);
            Some(home.join("itg-charter/target/release/itg-charter"))
        })
        .filter(|p| p.exists())
}

struct Env {
    root: PathBuf,
}

impl Env {
    /// Source media, a fake yt-dlp serving them, a logging ffmpeg wrapper and a fake
    /// itg-charter.
    fn new(name: &str) -> Env {
        let root = std::env::temp_dir().join(format!("itg-yt-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let s = |f: &str| src.join(f).to_string_lossy().into_owned();
        // 8 s of kick-like pulses at 120 BPM, so that the real itg-charter finds a beat.
        ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "aevalsrc=sin(2*PI*60*t)*exp(-30*mod(t\\,0.5)):s=44100:d=8",
            "-ac",
            "2",
            &s("audio.mp3"),
        ]);
        ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=25:duration=2",
            "-pix_fmt",
            "yuv420p",
            &s("video.mp4"),
        ]);
        ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=1280x720",
            "-frames:v",
            "1",
            &s("thumb.jpg"),
        ]);
        std::fs::write(
            src.join("info.json"),
            r#"{"id":"abc123","title":"Some Artist - Great Song (Official Video) [4K]","uploader":"SomeArtistVEVO"}"#,
        )
        .unwrap();
        // Fake yt-dlp: -J prints metadata; otherwise copies the requested media to the
        // -o template (with %(ext)s replaced). Every download is logged.
        write_script(
            &root.join("yt-dlp"),
            &format!(
                r#"#!/bin/sh
SRC='{src}'
out=""; fmt=""; thumb=0; prev=""
for a in "$@"; do
  case "$a" in -J) cat "$SRC/info.json"; exit 0;; --write-thumbnail) thumb=1;; esac
  [ "$prev" = "-o" ] && out="$a"
  [ "$prev" = "-f" ] && fmt="$a"
  prev="$a"
done
if [ $thumb = 1 ]; then f=thumb.jpg; ext=jpg
elif echo "$fmt" | grep -q '^bestaudio'; then f=audio.mp3; ext=mp3
else f=video.mp4; ext=mp4; fi
echo "$f" >> "$SRC/downloads.log"
cp "$SRC/$f" "$(echo "$out" | sed "s/%(ext)s/$ext/")"
"#,
                src = src.display()
            ),
        );
        // ffmpeg wrapper that logs every encode.
        write_script(
            &root.join("ffmpeg"),
            &format!(
                "#!/bin/sh\necho x >> '{src}/encodes.log'\nexec ffmpeg \"$@\"\n",
                src = src.display()
            ),
        );
        // Fake itg-charter: logs its calls; `gen` creates <out>/<title>/ with the audio,
        // the images and a simfile naming them; `decorate` adds the movie.
        write_script(
            &root.join("itg-charter"),
            &format!(
                r#"#!/bin/sh
echo "$@" >> '{src}/charter-calls.txt'
cmd="$1"; shift
if [ "$cmd" = gen ]; then
  audio="$1"; shift
  while [ $# -gt 0 ]; do
    case "$1" in
      -o) out="$2"; shift;;
      --title) title="$2"; shift;;
      --banner) banner="$2"; shift;;
      --background) background="$2"; shift;;
      --jacket) jacket="$2"; shift;;
    esac
    shift
  done
  dir="$out/$title"; mkdir -p "$dir"
  cp "$audio" "$banner" "$background" "$jacket" "$dir/"
  sm="$dir/$title.sm"
  printf '#TITLE:%s;\n#MUSIC:%s;\n#BANNER:%s;\n#BACKGROUND:%s;\n#JACKET:%s;\n#BGCHANGES:;\n' \
    "$title" "$(basename "$audio")" "$(basename "$banner")" "$(basename "$background")" "$(basename "$jacket")" > "$sm"
  echo "$sm"
elif [ "$cmd" = decorate ]; then
  sm="$1"; video="$3"
  cp "$video" "$(dirname "$sm")/"
  sed -i "s|#BGCHANGES:;|#BGCHANGES:$(basename "$video");|" "$sm"
  echo "$sm"
fi
"#,
                src = src.display()
            ),
        );
        Env { root }
    }

    fn run_with(&self, charter: &Path, out: &str, extra: &[&str]) -> PathBuf {
        let o = Command::new(env!("CARGO_BIN_EXE_itg-yt"))
            .arg("https://www.youtube.com/watch?v=abc123&list=RDxyz&index=3")
            .arg("-o")
            .arg(self.root.join(out))
            .arg("--cache")
            .arg(self.root.join("cache"))
            .args(extra)
            .env("ITG_YT_DLP", self.root.join("yt-dlp"))
            .env("ITG_FFMPEG", self.root.join("ffmpeg"))
            .env("ITG_CHARTER_BIN", charter)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        PathBuf::from(String::from_utf8(o.stdout).unwrap().trim())
    }

    fn run(&self, out: &str, extra: &[&str]) -> PathBuf {
        self.run_with(&self.root.join("itg-charter"), out, extra)
    }

    fn file(&self, f: &str) -> String {
        std::fs::read_to_string(self.root.join("src").join(f)).unwrap_or_default()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Small, fast video settings for the tests (the defaults are 1080p / medium).
const FAST: [&str; 4] = ["--video-height", "144", "--video-preset", "ultrafast"];

#[test]
fn builds_a_complete_song_folder() {
    if !has("ffmpeg") || !has("ffprobe") {
        eprintln!("ffmpeg not installed, skipping");
        return;
    }
    let env = Env::new("full");
    let sm = env.run("songs", &FAST);
    let dir = env.root.join("songs/Great Song");
    assert_eq!(sm, dir.join("Great Song.sm"));

    // gen with the defaults (all difficulties, seed 0), the cleaned names and the
    // images; then decorate with the movie.
    let calls = env.file("charter-calls.txt");
    let calls: Vec<&str> = calls.lines().collect();
    assert_eq!(calls.len(), 2, "{calls:?}");
    let gen_call = calls[0];
    assert!(
        gen_call.starts_with("gen ") && gen_call.contains("-d all -s 0"),
        "{gen_call}"
    );
    assert!(
        gen_call.contains("--title Great Song --artist Some Artist"),
        "{gen_call}"
    );
    assert!(
        gen_call.contains("bn.png") && gen_call.contains("jacket.png"),
        "{gen_call}"
    );
    assert!(!gen_call.contains("--no-stems"), "Demucs is on by default");
    assert!(
        calls[1].starts_with("decorate ") && calls[1].contains("--bg-video"),
        "{}",
        calls[1]
    );

    let ogg = probe(
        &dir.join("Great Song.ogg"),
        "stream=codec_name,sample_rate,channels",
    );
    assert!(
        ogg.contains("codec_name=vorbis")
            && ogg.contains("sample_rate=44100")
            && ogg.contains("channels=2"),
        "{ogg}"
    );
    let video = probe(
        &dir.join("Great Song-bg.mp4"),
        "stream=codec_type,codec_name,height",
    );
    assert!(
        video.contains("codec_name=h264") && video.contains("height=144"),
        "{video}"
    );
    assert!(
        !video.contains("codec_type=audio"),
        "background video must be silent"
    );
    for (png, w, h) in [
        ("bn.png", 418, 164),
        ("bg.png", 1920, 1080),
        ("jacket.png", 512, 512),
    ] {
        let p = probe(&dir.join(png), "stream=width,height");
        assert!(
            p.contains(&format!("width={w}")) && p.contains(&format!("height={h}")),
            "{png}: {p}"
        );
    }
    let text = std::fs::read_to_string(&sm).unwrap();
    assert!(text.contains("#BGCHANGES:Great Song-bg.mp4;"), "{text}");

    // A second run downloads and encodes nothing, and gives the same files.
    assert_eq!(
        env.file("downloads.log").lines().count(),
        3,
        "audio, video, thumbnail"
    );
    assert_eq!(
        env.file("encodes.log").lines().count(),
        5,
        "audio, 3 images, video"
    );
    let again = env.run("songs2", &FAST);
    assert_eq!(
        env.file("downloads.log").lines().count(),
        3,
        "nothing downloaded twice"
    );
    assert_eq!(
        env.file("encodes.log").lines().count(),
        5,
        "nothing encoded twice"
    );
    assert_eq!(std::fs::read(&sm).unwrap(), std::fs::read(&again).unwrap());
    let ogg2 = env.root.join("songs2/Great Song/Great Song.ogg");
    assert_eq!(
        std::fs::read(dir.join("Great Song.ogg")).unwrap(),
        std::fs::read(ogg2).unwrap()
    );

    // Another video quality is a new encode, the rest comes from the cache.
    env.run(
        "songs3",
        &["--video-height", "100", "--video-preset", "ultrafast"],
    );
    assert_eq!(
        env.file("encodes.log").lines().count(),
        6,
        "only the video is re-encoded"
    );
}

#[test]
fn video_is_never_upscaled() {
    if !has("ffmpeg") || !has("ffprobe") {
        return;
    }
    let env = Env::new("noupscale");
    // Default height is 1080 but the source is 180 pixels high.
    let sm = env.run("songs", &["--video-preset", "ultrafast", "-d", "easy"]);
    let video = probe(
        &sm.parent().unwrap().join("Great Song-bg.mp4"),
        "stream=height",
    );
    assert!(video.contains("height=180"), "{video}");
}

#[test]
fn no_video_option() {
    if !has("ffmpeg") || !has("ffprobe") {
        return;
    }
    let env = Env::new("novideo");
    let sm = env.run(
        "songs",
        &[
            "--no-video",
            "-d",
            "easy,hard",
            "--title",
            "X",
            "--artist",
            "Y",
            "--no-stems",
        ],
    );
    let dir = sm.parent().unwrap();
    assert!(!dir.join("X-bg.mp4").exists());
    assert!(!env.file("downloads.log").contains("video.mp4"));
    let calls = env.file("charter-calls.txt");
    assert_eq!(
        calls.lines().count(),
        1,
        "no decorate without a video: {calls}"
    );
    assert!(
        calls.contains("-d easy,hard") && calls.contains("--no-stems"),
        "{calls}"
    );
    assert!(
        std::fs::read_to_string(&sm)
            .unwrap()
            .contains("#BGCHANGES:;")
    );
}

/// With the real itg-charter (when installed): the folder is complete and the movie is
/// declared by itg-charter itself.
#[test]
fn with_the_real_itg_charter() {
    let Some(charter) = real_itg_charter() else {
        eprintln!("itg-charter not built, skipping");
        return;
    };
    if !has("ffmpeg") || !has("ffprobe") {
        return;
    }
    let env = Env::new("real");
    let mut extra = FAST.to_vec();
    extra.extend(["--no-stems", "-d", "easy,hard", "-s", "7"]);
    let sm = env.run_with(&charter, "songs", &extra);
    let dir = sm.parent().unwrap();
    for f in [
        "Great Song.ogg",
        "Great Song-bg.mp4",
        "bn.png",
        "bg.png",
        "jacket.png",
    ] {
        assert!(dir.join(f).exists(), "missing {f}");
    }
    let text = std::fs::read_to_string(&sm).unwrap();
    for tag in [
        "#BANNER:bn.png;",
        "#BACKGROUND:bg.png;",
        "#JACKET:jacket.png;",
        "#MUSIC:Great Song.ogg;",
    ] {
        assert!(text.contains(tag), "missing {tag}");
    }
    assert!(
        text.contains("=Great Song-bg.mp4=1.000=0=0=0=StretchNoLoop===="),
        "{text}"
    );
    assert_eq!(text.matches("#NOTES:").count(), 2);
}
