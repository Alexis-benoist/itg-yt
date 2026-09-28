//! External tools (yt-dlp, a JavaScript runtime for it, ffmpeg): where itg-yt finds
//! them, whether they work (checked before any download), and how to install them.
//!
//! Lookup order: the environment variable (`ITG_YT_DLP`, `ITG_FFMPEG`), then the tools
//! folder that `itg-yt setup` fills ([`bin_dir`]), then the usual places (the `PATH`).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const README: &str = "https://github.com/Alexis-benoist/itg-yt#installation";

/// Home folder: `$HOME`, or `%USERPROFILE%` on Windows.
pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
}

/// `name` with the executable suffix of the platform (`.exe` on Windows).
pub fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// Where an executable is in the PATH (also as `name.exe` on Windows).
pub fn find_in_path(name: &str) -> Option<PathBuf> {
    let names = [name.to_string(), exe(name)];
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .flat_map(|d| names.iter().map(move |n| d.join(n)))
        .find(|p| p.is_file())
}

pub fn in_path(name: &str) -> bool {
    find_in_path(name).is_some()
}

/// itg-yt's data folder: `~/.local/share/itg-yt` on Linux,
/// `~/Library/Application Support/itg-yt` on macOS, `%LOCALAPPDATA%\itg-yt` on Windows.
pub fn data_dir() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map_or_else(|| home().join("AppData").join("Local"), PathBuf::from)
            .join("itg-yt")
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support/itg-yt")
    } else {
        home().join(".local/share/itg-yt")
    }
}

/// Folder of the tools downloaded by `itg-yt setup`.
pub fn bin_dir() -> PathBuf {
    data_dir().join("bin")
}

/// A tool downloaded by `itg-yt setup`, if it is there.
pub fn managed(name: &str) -> Option<PathBuf> {
    let p = bin_dir().join(exe(name));
    p.is_file().then_some(p)
}

pub fn yt_dlp() -> PathBuf {
    if let Some(p) = std::env::var_os("ITG_YT_DLP") {
        return PathBuf::from(p);
    }
    let uv_tool = home().join(".local/bin").join(exe("yt-dlp"));
    managed("yt-dlp")
        .or_else(|| uv_tool.is_file().then_some(uv_tool))
        .unwrap_or_else(|| PathBuf::from("yt-dlp"))
}

pub fn ffmpeg() -> PathBuf {
    std::env::var_os("ITG_FFMPEG")
        .map(PathBuf::from)
        .or_else(|| managed("ffmpeg"))
        .unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

/// JavaScript runtime yt-dlp will use: (description, extra yt-dlp arguments).
fn js_runtime() -> Option<(String, Vec<OsString>)> {
    if let Some(deno) = managed("deno") {
        // Not in the PATH: tell yt-dlp where it is.
        let mut arg = OsString::from("deno:");
        arg.push(&deno);
        return Some((
            deno.display().to_string(),
            vec!["--js-runtimes".into(), arg],
        ));
    }
    if let Some(deno) = find_in_path("deno") {
        return Some((deno.display().to_string(), vec![]));
    }
    // yt-dlp only enables deno by default.
    find_in_path("node").map(|node| {
        (
            node.display().to_string(),
            vec!["--js-runtimes".into(), "node".into()],
        )
    })
}

/// Arguments that make yt-dlp use the same JavaScript runtime and ffmpeg as itg-yt
/// (they may be in the tools folder, which is not in the PATH).
pub fn yt_dlp_tool_args() -> Vec<OsString> {
    let mut args = js_runtime().map(|r| r.1).unwrap_or_default();
    let ff = ffmpeg();
    if ff != Path::new("ffmpeg") {
        args.push("--ffmpeg-location".into());
        args.push(ff.into());
    }
    args
}

/// The state of one required tool.
pub struct Check {
    pub name: &'static str,
    pub purpose: &'static str,
    /// What was found (path, version), or why it is not usable.
    pub result: Result<String, String>,
    /// The tool comes from an environment variable: `setup` must not replace it.
    pub env_var: Option<&'static str>,
}

impl Check {
    pub fn ok(&self) -> bool {
        self.result.is_ok()
    }
}

/// Runs `cmd` and returns its standard output if it succeeds.
fn run(mut cmd: Command) -> Result<String, String> {
    let prog = cmd.get_program().to_owned();
    let out = cmd
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "not found".to_string()
            } else {
                format!("{} cannot be started ({e})", Path::new(&prog).display())
            }
        })?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "{} does not work ({})",
            Path::new(&prog).display(),
            err.lines().last().unwrap_or("no output").trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn env_var(name: &'static str) -> Option<&'static str> {
    std::env::var_os(name).map(|_| name)
}

/// yt-dlp: only checks that the program is there. Starting the standalone yt-dlp takes
/// several seconds (it unpacks itself), too long before every run; if it is there but
/// cannot start, the metadata call reports it ([`yt_dlp_cannot_start`]).
pub fn check_yt_dlp_exists() -> Check {
    let path = yt_dlp();
    let found = if path.components().count() > 1 {
        path.is_file().then_some(path)
    } else {
        find_in_path(&path.to_string_lossy())
    };
    yt_dlp_check(
        found
            .map(|p| p.display().to_string())
            .ok_or_else(|| "not found".to_string()),
    )
}

/// yt-dlp was found but could not be started.
pub fn yt_dlp_cannot_start(e: &std::io::Error) -> Check {
    yt_dlp_check(Err(format!(
        "{} cannot be started ({e})",
        yt_dlp().display()
    )))
}

fn yt_dlp_check(result: Result<String, String>) -> Check {
    Check {
        name: "yt-dlp",
        purpose: "downloads the videos from YouTube",
        result,
        env_var: env_var("ITG_YT_DLP"),
    }
}

pub fn check_js_runtime() -> Check {
    Check {
        name: "deno or node",
        purpose: "JavaScript runtime yt-dlp needs to solve YouTube's challenges",
        result: js_runtime()
            .map(|r| r.0)
            .ok_or_else(|| "not found".to_string()),
        env_var: None,
    }
}

/// Encoders itg-yt uses: libvorbis (audio), and libx264 (background video) when `video`.
pub fn check_ffmpeg(video: bool) -> Check {
    let path = ffmpeg();
    let mut cmd = Command::new(&path);
    cmd.args(["-hide_banner", "-encoders"]);
    let needed: &[&str] = if video {
        &["libvorbis", "libx264"]
    } else {
        &["libvorbis"]
    };
    let result = run(cmd).and_then(|list| {
        let has = |enc: &str| {
            list.lines()
                .any(|l| l.split_whitespace().nth(1) == Some(enc))
        };
        let missing: Vec<&str> = needed.iter().copied().filter(|e| !has(e)).collect();
        if missing.is_empty() {
            Ok(path.display().to_string())
        } else {
            Err(format!(
                "{} lacks the {} encoder{} (incomplete build)",
                path.display(),
                missing.join(" and "),
                if missing.len() > 1 { "s" } else { "" }
            ))
        }
    });
    Check {
        name: "ffmpeg",
        purpose: if video {
            "encodes the audio with libvorbis and the background video with libx264"
        } else {
            "encodes the audio with libvorbis"
        },
        result,
        env_var: env_var("ITG_FFMPEG"),
    }
}

/// Every required tool, in the order they are used.
/// Every required tool, in the order they are used; quick checks, before every run.
pub fn check_all(video: bool) -> Vec<Check> {
    vec![
        check_yt_dlp_exists(),
        check_js_runtime(),
        check_ffmpeg(video),
    ]
}

/// How to install a tool by hand on this system.
pub fn manual_install(name: &str) -> &'static str {
    let (linux, macos, windows) = match name {
        "yt-dlp" => (
            "uv tool install \"yt-dlp[default]\"   (uv: https://docs.astral.sh/uv/)",
            "uv tool install \"yt-dlp[default]\"   (uv: https://docs.astral.sh/uv/)",
            "uv tool install \"yt-dlp[default]\"   (uv: winget install astral-sh.uv)",
        ),
        "ffmpeg" => (
            "sudo apt install ffmpeg (Debian/Ubuntu), sudo dnf install ffmpeg (Fedora, RPM Fusion), sudo pacman -S ffmpeg (Arch)",
            "brew install ffmpeg",
            "winget install Gyan.FFmpeg",
        ),
        _ => (
            "curl -fsSL https://deno.land/install.sh | sh   (or: sudo apt install nodejs)",
            "brew install deno",
            "winget install DenoLand.Deno",
        ),
    };
    if cfg!(windows) {
        windows
    } else if cfg!(target_os = "macos") {
        macos
    } else {
        linux
    }
}

/// Message for the tools that are not usable (empty when everything is fine).
pub fn report_problems(checks: &[Check]) -> String {
    let bad: Vec<&Check> = checks.iter().filter(|c| !c.ok()).collect();
    if bad.is_empty() {
        return String::new();
    }
    let mut s = String::from("error: some tools itg-yt needs are missing or incomplete:\n");
    for c in &bad {
        let why = c.result.as_ref().err().map_or("", String::as_str);
        s += &format!("\n  - {} ({}): {why}\n", c.name, c.purpose);
        match c.env_var {
            Some(var) => s += &format!("      fix or unset the {var} environment variable\n"),
            None => s += &format!("      to install it by hand: {}\n", manual_install(c.name)),
        }
    }
    s += &format!(
        "\nEasiest: run `itg-yt setup`, which downloads what is missing into\n  {}\n\
         See {README}\n",
        bin_dir().display()
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problems_name_the_tool_and_the_fix() {
        let checks = [
            Check {
                name: "yt-dlp",
                purpose: "downloads",
                result: Err("not found".into()),
                env_var: None,
            },
            Check {
                name: "ffmpeg",
                purpose: "encodes",
                result: Err("bad".into()),
                env_var: Some("ITG_FFMPEG"),
            },
            Check {
                name: "deno or node",
                purpose: "js",
                result: Ok("/usr/bin/node".into()),
                env_var: None,
            },
        ];
        let r = report_problems(&checks);
        assert!(r.contains("yt-dlp (downloads): not found"), "{r}");
        assert!(r.contains("uv tool install"), "{r}");
        assert!(r.contains("unset the ITG_FFMPEG"), "{r}");
        assert!(!r.contains("deno"), "{r}");
        assert!(r.contains("itg-yt setup"), "{r}");
        assert!(report_problems(&checks[2..]).is_empty());
    }
}
