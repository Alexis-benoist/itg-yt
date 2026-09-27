//! Offline end-to-end test: fake yt-dlp and fake itg-charter (shell scripts), real ffmpeg.
//! Skipped when ffmpeg/ffprobe are not installed.

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

struct Env {
    root: PathBuf,
}

impl Env {
    /// Source media, a fake yt-dlp serving them, and a fake itg-charter.
    fn new(name: &str) -> Env {
        let root = std::env::temp_dir().join(format!("itg-yt-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let s = |f: &str| src.join(f).to_string_lossy().into_owned();
        ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=4",
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
        // Fake itg-charter: records its arguments and writes a minimal simfile next to
        // the audio, like the real one (constant BPM 120, offset -0.1).
        write_script(
            &root.join("itg-charter"),
            &format!(
                r#"#!/bin/sh
echo "$@" > '{src}/charter-args.txt'
ogg="$2"; dir=$(dirname "$ogg"); name=$(basename "$dir")
cat > "$dir/$name.sm" <<EOF
#TITLE:$name;
#BANNER:;
#BACKGROUND:;
#MUSIC:$(basename "$ogg");
#OFFSET:-0.100;
#BPMS:0.000=120.000;
#BGCHANGES:;

//---------------dance-single - fake----------------
#NOTES:
     dance-single:
     fake:
     Easy:
     1:
     0.000,0.000,0.000,0.000,0.000:
1000
;
EOF
echo "$dir/$name.sm"
"#,
                src = src.display()
            ),
        );
        Env { root }
    }

    fn run(&self, out: &str, extra: &[&str]) -> PathBuf {
        let o = Command::new(env!("CARGO_BIN_EXE_itg-yt"))
            .arg("https://www.youtube.com/watch?v=abc123")
            .arg("-o")
            .arg(self.root.join(out))
            .arg("--cache")
            .arg(self.root.join("cache"))
            .args(extra)
            .env("ITG_YT_DLP", self.root.join("yt-dlp"))
            .env("ITG_CHARTER_BIN", self.root.join("itg-charter"))
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        PathBuf::from(String::from_utf8(o.stdout).unwrap().trim())
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

#[test]
fn builds_a_complete_song_folder() {
    if !has("ffmpeg") || !has("ffprobe") {
        eprintln!("ffmpeg not installed, skipping");
        return;
    }
    let env = Env::new("full");
    // Small, fast video settings for the test (the defaults are 1080p / medium).
    const FAST: [&str; 4] = ["--video-height", "144", "--video-preset", "ultrafast"];
    let sm = env.run("songs", &FAST);
    let dir = env.root.join("songs/Great Song");
    assert_eq!(sm, dir.join("Great Song.sm"));

    // Defaults passed to itg-charter: all difficulties, seed 0, cleaned title/artist.
    let args = env.file("charter-args.txt");
    assert!(args.contains("-d all") && args.contains("-s 0"), "{args}");
    assert!(
        args.contains("--title Great Song --artist Some Artist"),
        "{args}"
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
    let mp4 = dir.join("Great Song-bg.mp4");
    let video = probe(&mp4, "stream=codec_type,codec_name,height");
    assert!(
        video.contains("codec_name=h264") && video.contains("height=144"),
        "{video}"
    );
    assert!(
        !video.contains("codec_type=audio"),
        "background video must be silent"
    );
    for (png, size) in [
        ("bn.png", "418x164"),
        ("bg.png", "1920x1080"),
        ("jacket.png", "512x512"),
    ] {
        let p = probe(&dir.join(png), "stream=width,height");
        let (w, h) = size.split_once('x').unwrap();
        assert!(
            p.contains(&format!("width={w}")) && p.contains(&format!("height={h}")),
            "{png}: {p}"
        );
    }

    let text = std::fs::read_to_string(&sm).unwrap();
    for tag in [
        "#BANNER:bn.png;",
        "#BACKGROUND:bg.png;",
        "#JACKET:jacket.png;",
        // offset -0.1 s at 120 BPM: audio time 0 is beat -0.2
        "#BGCHANGES:-0.200=Great Song-bg.mp4=1.000=0=0=0=StretchNoLoop====;",
    ] {
        assert!(text.contains(tag), "missing {tag} in\n{text}");
    }

    // A second run reuses the download cache and gives the same files.
    let downloads = env.file("downloads.log");
    assert_eq!(
        downloads.lines().count(),
        3,
        "audio, video and thumbnail: {downloads}"
    );
    let again = env.run("songs2", &FAST);
    assert_eq!(
        env.file("downloads.log"),
        downloads,
        "nothing downloaded twice"
    );
    assert_eq!(std::fs::read(&sm).unwrap(), std::fs::read(&again).unwrap());
    let ogg2 = env.root.join("songs2/Great Song/Great Song.ogg");
    assert_eq!(
        std::fs::read(dir.join("Great Song.ogg")).unwrap(),
        std::fs::read(ogg2).unwrap()
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
        ],
    );
    let dir = sm.parent().unwrap();
    assert!(!dir.join("X-bg.mp4").exists());
    assert!(!env.file("downloads.log").contains("video.mp4"));
    let text = std::fs::read_to_string(&sm).unwrap();
    assert!(text.contains("#BGCHANGES:;") && text.contains("#JACKET:jacket.png;"));
    assert!(env.file("charter-args.txt").contains("-d easy,hard"));
}
