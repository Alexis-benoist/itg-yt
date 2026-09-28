//! `itg-yt setup`: everything a newcomer needs, in one command.
//!
//! 1. Downloads the missing tools (yt-dlp, deno, ffmpeg) from their official release
//!    pages into itg-yt's tools folder ([`tools::bin_dir`]), where itg-yt looks after the
//!    environment variables and before the `PATH`. Every download is checked against the
//!    SHA-256 published with the release.
//! 2. Links the output folder (`~/ITG-YouTube`) into ITGmania's `Songs` folder as
//!    `YouTube` (symbolic link on Unix, junction on Windows), never replacing an existing
//!    folder.
//! 3. Optionally (`--demucs`) creates the Demucs Python environment with uv.
//!
//! Downloads and archives go through the system's `curl` and `tar` (both shipped with
//! Windows 10+ and macOS; on Linux `unzip` or Python for zip files), which keeps
//! itg-yt free of HTTP/TLS/archive crates and uses the system's certificates.

use crate::tools::{self, Check, bin_dir, exe, find_in_path, home, managed};
use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(clap::Args)]
pub struct SetupArgs {
    /// Only report what is installed and what is missing: downloads nothing, changes
    /// nothing (exit code 2 if a required tool is missing, 1 if the link is missing).
    #[arg(long)]
    check: bool,
    /// Downloads again the tools installed by setup (a newer yt-dlp is the first thing to
    /// try when YouTube downloads start failing).
    #[arg(long)]
    update: bool,
    /// ITGmania's Songs folder (default: the one of a regular install). For a portable
    /// install: <ITGmania folder>/Songs.
    #[arg(long)]
    songs: Option<PathBuf>,
    /// Output folder to link into the game (default: ~/ITG-YouTube, where itg-yt writes).
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Also installs Demucs (optional, better charts) with uv: downloads PyTorch, several GB.
    #[arg(long)]
    demucs: bool,
}

/// Runs `itg-yt setup`; returns the exit code: 0 when everything is ready, 2 when a
/// required tool is still missing, 1 when something else is left to do (printed).
pub fn run(a: &SetupArgs) -> i32 {
    println!("Tools folder: {}", bin_dir().display());
    let mut tools_ok = true;
    for (tool, check) in [
        ("yt-dlp", tools::check_yt_dlp as fn() -> Check),
        ("deno", tools::check_js_runtime),
        ("ffmpeg", || tools::check_ffmpeg(true)),
    ] {
        tools_ok &= ensure_tool(a, tool, check);
    }

    println!("\nITGmania:");
    let linked = link_songs(a).unwrap_or_else(|e| {
        println!("  FAILED   {e:#}");
        false
    });

    println!("\nOptional:");
    let demucs_ok = demucs(a);

    println!();
    if !tools_ok {
        println!(
            "Some required tools are missing (see above; manual installation: {}).",
            tools::README
        );
        2
    } else if !linked || !demucs_ok {
        println!("itg-yt works, but see above for what is left to do.");
        1
    } else {
        if !a.check {
            println!(
                "Ready. Try: itg-yt --no-video \"https://www.youtube.com/watch?v=...\"\n\
                 then (re)load songs in ITGmania: the song is in the \"YouTube\" pack."
            );
        }
        0
    }
}

/// Checks one tool and downloads it if it is missing (or with `--update`).
fn ensure_tool(a: &SetupArgs, tool: &str, check: fn() -> Check) -> bool {
    let c = check();
    let refresh = a.update && managed(tool).is_some() && c.env_var.is_none();
    match &c.result {
        Ok(found) if !refresh => {
            println!("  ok       {}: {found}", c.name);
            return true;
        }
        Ok(found) => println!("  update   {}: {found}", c.name),
        Err(why) => println!("  missing  {} ({}): {why}", c.name, c.purpose),
    }
    if a.check {
        return c.ok();
    }
    if let Some(var) = c.env_var {
        println!("           it comes from {var}: fix or unset that variable");
        return false;
    }
    let installed = downloads(tool)
        .map_err(anyhow::Error::msg)
        .and_then(|d| install(tool, &d));
    if let Err(e) = installed {
        println!("  FAILED   {tool}: {e:#}");
        println!(
            "           install it by hand: {}",
            tools::manual_install(tool)
        );
        return false;
    }
    let c = check();
    match &c.result {
        Ok(found) => println!("  ok       {}: {found}", c.name),
        Err(why) => println!("  FAILED   {} after installation: {why}", c.name),
    }
    c.ok()
}

/// Kind of a downloaded file.
#[derive(Debug, PartialEq)]
enum Kind {
    /// The program itself, installed under this name.
    Binary(&'static str),
    Zip,
    TarXz,
}

/// Where the SHA-256 of a download is published.
#[derive(Debug, PartialEq)]
enum Sums {
    /// A file listing the checksums of every asset of the release.
    List(&'static str),
    /// A file next to the download (its URL + this suffix).
    Sidecar(&'static str),
    /// A file next to the download, at its final URL after redirects + this suffix (for
    /// redirection services that only know the downloads themselves).
    SidecarOfFinal(&'static str),
}

#[derive(Debug, PartialEq)]
struct Download {
    url: String,
    sums: Sums,
    kind: Kind,
}

/// Official downloads of `tool` for this system (`Err` when there is none).
fn downloads(tool: &str) -> Result<Vec<Download>, String> {
    downloads_for(tool, std::env::consts::OS, std::env::consts::ARCH)
}

fn downloads_for(tool: &str, os: &str, arch: &str) -> Result<Vec<Download>, String> {
    let unsupported = || Err(format!("no download for {os} {arch}"));
    let d = match tool {
        "yt-dlp" => {
            let asset = match (os, arch) {
                ("linux", "x86_64") => "yt-dlp_linux",
                ("linux", "aarch64") => "yt-dlp_linux_aarch64",
                ("macos", _) => "yt-dlp_macos",
                ("windows", "x86_64") => "yt-dlp.exe",
                ("windows", "aarch64") => "yt-dlp_arm64.exe",
                _ => return unsupported(),
            };
            vec![Download {
                url: format!("https://github.com/yt-dlp/yt-dlp/releases/latest/download/{asset}"),
                sums: Sums::List(
                    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS",
                ),
                kind: Kind::Binary("yt-dlp"),
            }]
        }
        "deno" => {
            let target = match (os, arch) {
                ("linux", "x86_64" | "aarch64") => format!("{arch}-unknown-linux-gnu"),
                ("macos", "x86_64" | "aarch64") => format!("{arch}-apple-darwin"),
                ("windows", "x86_64" | "aarch64") => format!("{arch}-pc-windows-msvc"),
                _ => return unsupported(),
            };
            vec![Download {
                url: format!(
                    "https://github.com/denoland/deno/releases/latest/download/deno-{target}.zip"
                ),
                sums: Sums::Sidecar(".sha256sum"),
                kind: Kind::Zip,
            }]
        }
        "ffmpeg" => {
            const BTBN: &str = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest";
            const BTBN_SUMS: &str =
                "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/checksums.sha256";
            match (os, arch) {
                ("linux", "x86_64" | "aarch64") => {
                    let a = if arch == "x86_64" { "64" } else { "arm64" };
                    vec![Download {
                        url: format!("{BTBN}/ffmpeg-master-latest-linux{a}-gpl.tar.xz"),
                        sums: Sums::List(BTBN_SUMS),
                        kind: Kind::TarXz,
                    }]
                }
                ("windows", "x86_64" | "aarch64") => {
                    let a = if arch == "x86_64" { "64" } else { "arm64" };
                    vec![Download {
                        url: format!("{BTBN}/ffmpeg-master-latest-win{a}-gpl.zip"),
                        sums: Sums::List(BTBN_SUMS),
                        kind: Kind::Zip,
                    }]
                }
                // Static builds by Martin Riedl (with libvorbis and libx264), one zip
                // per program.
                ("macos", "x86_64" | "aarch64") => {
                    let a = if arch == "x86_64" { "amd64" } else { "arm64" };
                    ["ffmpeg", "ffprobe"]
                        .map(|p| Download {
                            url: format!(
                                "https://ffmpeg.martin-riedl.de/redirect/latest/macos/{a}/release/{p}.zip"
                            ),
                            sums: Sums::SidecarOfFinal(".sha256"),
                            kind: Kind::Zip,
                        })
                        .into()
                }
                _ => return unsupported(),
            }
        }
        _ => return unsupported(),
    };
    Ok(d)
}

/// Programs to take from the downloads of `tool`.
fn programs(tool: &str) -> &'static [&'static str] {
    match tool {
        "ffmpeg" => &["ffmpeg", "ffprobe"],
        "deno" => &["deno"],
        _ => &["yt-dlp"],
    }
}

/// A system program: the one of `System32` on Windows (another `tar`, e.g. Git's, may be
/// first in the PATH and not read zip files).
fn system_tool(name: &str) -> Command {
    if cfg!(windows)
        && let Some(root) = std::env::var_os("SystemRoot")
    {
        let p = Path::new(&root).join("System32").join(exe(name));
        if p.is_file() {
            return Command::new(p);
        }
    }
    Command::new(name)
}

fn curl() -> Command {
    let mut c = system_tool("curl");
    // HTTPS only, also after redirects.
    c.args(["--proto", "=https", "--fail", "--location", "--retry", "3"]);
    c
}

fn run_ok(mut cmd: Command) -> Result<String> {
    let prog = cmd.get_program().to_string_lossy().into_owned();
    let out = cmd
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("cannot start {prog}"))?;
    ensure!(out.status.success(), "{prog} failed ({})", out.status);
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Downloads `url` into `file` (progress bar on stderr); returns the final URL.
fn download(url: &str, file: &Path) -> Result<String> {
    println!("           downloading {url}");
    let mut c = curl();
    c.args([
        "--progress-bar",
        "--write-out",
        "%{url_effective}",
        "--output",
    ])
    .arg(file)
    .arg(url);
    run_ok(c).with_context(|| format!("downloading {url}"))
}

fn fetch_text(url: &str) -> Result<String> {
    let mut c = curl();
    c.args(["--silent", "--show-error", url]);
    run_ok(c).with_context(|| format!("downloading {url}"))
}

/// The SHA-256 of `asset` in a checksum file: the one on the line naming the asset, or
/// the only one of the file (lower case).
fn expected_sha256(text: &str, asset: &str) -> Option<String> {
    let hash = |line: &str| {
        line.split(|c: char| !c.is_ascii_alphanumeric())
            .find(|w| w.len() == 64 && w.chars().all(|c| c.is_ascii_hexdigit()))
            .map(str::to_ascii_lowercase)
    };
    let named = text.lines().find(|l| {
        l.split_whitespace()
            .any(|w| w.trim_start_matches('*') == asset)
    });
    if let Some(line) = named {
        return hash(line);
    }
    let all: Vec<String> = text.lines().filter_map(hash).collect();
    (all.len() == 1).then(|| all[0].clone())
}

fn sha256_file(file: &Path) -> Result<String> {
    let mut f = std::fs::File::open(file)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Last segment of a URL.
fn url_file(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or(url)
}

fn extract(archive: &Path, kind: &Kind, out: &Path) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let mut c;
    if *kind == Kind::TarXz {
        c = system_tool("tar");
        c.arg("-xJf").arg(archive).arg("-C").arg(out);
    } else if !cfg!(target_os = "linux") {
        // The bsdtar of macOS and Windows 10+ reads zip files.
        c = system_tool("tar");
        c.arg("-xf").arg(archive).arg("-C").arg(out);
    } else if tools::in_path("unzip") {
        c = Command::new("unzip");
        c.arg("-q").arg("-o").arg(archive).arg("-d").arg(out);
    } else {
        c = Command::new("python3");
        c.args(["-m", "zipfile", "-e"]).arg(archive).arg(out);
    }
    run_ok(c)
        .map(|_| ())
        .with_context(|| format!("extracting {}", archive.display()))
}

/// First file called `name` under `dir`.
fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    entries
        .iter()
        .find(|p| p.is_file() && p.file_name().is_some_and(|n| n == name))
        .cloned()
        .or_else(|| {
            entries
                .iter()
                .filter(|p| p.is_dir())
                .find_map(|d| find_file(d, name))
        })
}

/// Makes `src` executable and moves it to `dst` (replacing it).
fn place(src: &Path, dst: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(src, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(src, dst)
        .with_context(|| format!("moving {} to {}", src.display(), dst.display()))
}

/// Downloads, verifies and installs `tool` into the tools folder.
fn install(tool: &str, downloads: &[Download]) -> Result<()> {
    let bin = bin_dir();
    let staging = bin.join(format!(".setup-{tool}"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).with_context(|| format!("creating {}", staging.display()))?;
    let result = (|| {
        for (i, d) in downloads.iter().enumerate() {
            let file = staging.join(url_file(&d.url));
            let url = download(&d.url, &file)?;
            let (sums_url, asset) = match d.sums {
                Sums::List(list) => (list.to_string(), url_file(&d.url).to_string()),
                Sums::Sidecar(suffix) => {
                    (format!("{}{suffix}", d.url), url_file(&d.url).to_string())
                }
                Sums::SidecarOfFinal(suffix) => {
                    (format!("{url}{suffix}"), url_file(&url).to_string())
                }
            };
            let expected = expected_sha256(&fetch_text(&sums_url)?, &asset)
                .with_context(|| format!("no SHA-256 of {asset} in {sums_url}"))?;
            let got = sha256_file(&file)?;
            ensure!(
                got == expected,
                "{asset}: SHA-256 mismatch (expected {expected}, got {got}): download refused"
            );
            println!("           SHA-256 verified ({sums_url})");
            match d.kind {
                Kind::Binary(name) => place(&file, &bin.join(exe(name)))?,
                ref kind => {
                    let out = staging.join(format!("x{i}"));
                    extract(&file, kind, &out)?;
                    for p in programs(tool) {
                        if let Some(f) = find_file(&out, &exe(p)) {
                            place(&f, &bin.join(exe(p)))?;
                        }
                    }
                }
            }
        }
        for p in programs(tool) {
            if managed(p).is_none() {
                bail!("{} not found in the download", exe(p));
            }
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// ITGmania's Songs folder for a regular install.
fn default_songs() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map_or_else(|| home().join("AppData").join("Roaming"), PathBuf::from)
            .join("ITGmania")
            .join("Songs")
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support/ITGmania/Songs")
    } else {
        home().join(".itgmania/Songs")
    }
}

/// Whether two paths are the same folder (following links).
fn same_dir(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(unix)]
fn make_link(link: &Path, target: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link)
        .with_context(|| format!("creating the link {}", link.display()))
}

/// A junction: unlike a symbolic link, it needs no administrator rights.
#[cfg(not(unix))]
fn make_link(link: &Path, target: &Path) -> Result<()> {
    let mut c = Command::new("cmd");
    c.args(["/C", "mklink", "/J"]).arg(link).arg(target);
    run_ok(c)
        .map(|_| ())
        .with_context(|| format!("creating the junction {}", link.display()))
}

/// Links the output folder into the Songs folder as "YouTube"; true when the link is there.
fn link_songs(a: &SetupArgs) -> Result<bool> {
    let target = a
        .output
        .clone()
        .unwrap_or_else(|| home().join("ITG-YouTube"));
    let songs = a.songs.clone().unwrap_or_else(default_songs);
    if !songs.is_dir() {
        println!(
            "  missing  ITGmania Songs folder: {} not found",
            songs.display()
        );
        if a.songs.is_none() {
            println!(
                "           start ITGmania once, or for a portable install pass\n           \
                 itg-yt setup --songs \"<ITGmania folder>/Songs\""
            );
        }
        return Ok(false);
    }
    let link = songs.join("YouTube");
    if !a.check {
        std::fs::create_dir_all(&target)
            .with_context(|| format!("creating {}", target.display()))?;
    }
    if std::fs::symlink_metadata(&link).is_ok() {
        if same_dir(&link, &target) {
            println!("  ok       {} -> {}", link.display(), target.display());
            return Ok(true);
        }
        println!(
            "  conflict {} already exists and is not a link to {}: left untouched.\n           \
             Remove it and run `itg-yt setup` again, or write songs straight into it:\n           \
             itg-yt -o \"{}\" URL",
            link.display(),
            target.display(),
            link.display()
        );
        return Ok(false);
    }
    if a.check {
        println!("  missing  link {} -> {}", link.display(), target.display());
        return Ok(false);
    }
    make_link(&link, &target)?;
    println!("  linked   {} -> {}", link.display(), target.display());
    Ok(true)
}

/// Python of the Demucs environment, as itg-charter looks for it.
fn demucs_python() -> PathBuf {
    std::env::var_os("ITG_CHARTER_PYTHON").map_or_else(
        || home().join(".local/share/itg-charter/demucs-venv/bin/python"),
        PathBuf::from,
    )
}

fn find_uv() -> Option<PathBuf> {
    find_in_path("uv").or_else(|| {
        [".local/bin", ".cargo/bin"]
            .iter()
            .map(|d| home().join(d).join(exe("uv")))
            .find(|p| p.is_file())
    })
}

/// Reports Demucs and installs it with `--demucs`; false when asked for and not done.
fn demucs(a: &SetupArgs) -> bool {
    let python = demucs_python();
    if python.is_file() {
        println!("  ok       Demucs (better charts): {}", python.display());
        return true;
    }
    if !a.demucs || a.check {
        println!(
            "  --       Demucs (better charts): not installed, charts use the full mix.\n           \
             To install it (downloads PyTorch, several GB): itg-yt setup --demucs"
        );
        return !a.demucs;
    }
    if std::env::var_os("ITG_CHARTER_PYTHON").is_some() {
        println!(
            "  missing  Demucs: ITG_CHARTER_PYTHON points to {}, which does not exist: fix or unset it",
            python.display()
        );
        return false;
    }
    let Some(uv) = find_uv() else {
        println!(
            "  missing  Demucs: uv is needed to install it (https://docs.astral.sh/uv/);\n           \
             see {}",
            tools::README
        );
        return false;
    };
    match install_demucs(&uv) {
        Ok(()) => true,
        Err(e) => {
            println!("  FAILED   Demucs: {e:#}");
            false
        }
    }
}

fn install_demucs(uv: &Path) -> Result<()> {
    let windows = cfg!(windows);
    // On Windows the default path of itg-charter (…/bin/python) cannot exist: the venv
    // goes where the README puts it and ITG_CHARTER_PYTHON is set for the user.
    let venv = if windows {
        home().join("itg-charter").join("demucs-venv")
    } else {
        home().join(".local/share/itg-charter/demucs-venv")
    };
    let step = |args: &[&str]| -> Result<()> {
        println!("           uv {}", args.join(" "));
        let st = Command::new(uv)
            .args(args)
            .env("VIRTUAL_ENV", &venv)
            .status()
            .context("cannot start uv")?;
        ensure!(st.success(), "uv {} failed", args.join(" "));
        Ok(())
    };
    let venv_str = venv.to_string_lossy().into_owned();
    step(&["venv", "--python", "3.11", &venv_str])?;
    if windows {
        if tools::in_path("nvidia-smi") {
            // NVIDIA GPU: torch with CUDA (the default Windows wheels are CPU only).
            step(&[
                "pip",
                "install",
                "torch",
                "torchaudio",
                "--index-url",
                "https://download.pytorch.org/whl/cu124",
            ])?;
        }
        step(&[
            "pip",
            "install",
            "demucs",
            "torch",
            "torchaudio",
            "soundfile",
        ])?;
        let python = venv.join("Scripts").join("python.exe");
        let mut c = Command::new("setx");
        c.arg("ITG_CHARTER_PYTHON").arg(&python);
        run_ok(c)?;
        println!(
            "  ok       Demucs: {} (ITG_CHARTER_PYTHON set: open a new terminal)",
            python.display()
        );
    } else {
        step(&[
            "pip",
            "install",
            "demucs",
            "torch",
            "torchaudio",
            "soundfile",
        ])?;
        println!(
            "  ok       Demucs: {}",
            venv.join("bin").join("python").display()
        );
    }
    if !tools::in_path("nvidia-smi") {
        println!("           no NVIDIA GPU found: run itg-yt with --device cpu");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_are_found_in_every_format() {
        let h = "c6527f24f4b16031d3ae4fa9f658d5f11534c8d84ce7dc8502420280919c3490";
        // yt-dlp's SHA2-256SUMS / BtbN's checksums.sha256: one line per asset.
        let list = format!(
            "{}  yt-dlp\n{h}  yt-dlp_linux\n{}  yt-dlp_linux.zip\n",
            "1".repeat(64),
            "2".repeat(64)
        );
        assert_eq!(expected_sha256(&list, "yt-dlp_linux"), Some(h.into()));
        assert_eq!(expected_sha256(&list, "yt-dlp_macos"), None);
        // deno on Linux / macOS, Martin Riedl: one line.
        let one = format!("{h}  deno-x86_64-unknown-linux-gnu.zip\n");
        assert_eq!(
            expected_sha256(&one, "deno-x86_64-unknown-linux-gnu.zip"),
            Some(h.into())
        );
        // deno on Windows: PowerShell's Get-FileHash output, upper case.
        let ps = format!(
            "\r\nAlgorithm : SHA256\r\nHash      : {}\r\nPath      : C:\\a\\deno\\deno\\target\\release\\deno-x86_64-pc-windows-msvc.zip\r\n",
            h.to_uppercase()
        );
        assert_eq!(
            expected_sha256(&ps, "deno-x86_64-pc-windows-msvc.zip"),
            Some(h.into())
        );
    }

    #[test]
    fn downloads_exist_for_the_release_platforms() {
        for (os, arch) in [
            ("linux", "x86_64"),
            ("macos", "aarch64"),
            ("macos", "x86_64"),
            ("windows", "x86_64"),
        ] {
            for tool in ["yt-dlp", "deno", "ffmpeg"] {
                let d = downloads_for(tool, os, arch).unwrap();
                assert!(
                    d.iter().all(|d| d.url.starts_with("https://")),
                    "{tool} {os}"
                );
            }
        }
        let ff = downloads_for("ffmpeg", "linux", "x86_64").unwrap();
        assert_eq!(
            url_file(&ff[0].url),
            "ffmpeg-master-latest-linux64-gpl.tar.xz"
        );
        assert_eq!(
            downloads_for("ffmpeg", "macos", "aarch64").unwrap().len(),
            2
        );
        assert!(downloads_for("yt-dlp", "freebsd", "x86_64").is_err());
    }
}
