# itg-yt

Creates a complete **ITGmania / In The Groove** song folder from a YouTube video: charts (by
[itg-charter](https://github.com/Alexis-benoist/itg-charter)), OGG audio, banner, background,
jacket and the music video as a background movie.

```sh
itg-yt "https://www.youtube.com/watch?v=..."
itg-yt URL1 URL2 URL3                 # several songs
itg-yt "https://www.youtube.com/playlist?list=..."   # a whole playlist
itg-yt -a my-songs.txt                # one URL per line (# = comment, - = stdin)
```

- [Installation](#installation): itg-yt, ffmpeg, yt-dlp, Demucs, hooking into the game
- [Usage](#usage) · [Options](#options) · [How it works](#how-it-works)
- [Troubleshooting](#troubleshooting)

## Installation

### What you need

| tool | purpose | required? |
|---|---|---|
| **itg-yt** | this program (itg-charter is built into it: nothing else to install for the charts) | yes |
| **ffmpeg** with libvorbis and libx264 | encodes the audio (OGG), the images and the video | yes |
| **yt-dlp** with its EJS scripts | downloads from YouTube | yes |
| **deno** or **node** | JavaScript runtime yt-dlp needs to solve YouTube challenges | yes |
| **Python 3.11 + Demucs** | separates drums / bass / vocals to place notes better | no (without it: a warning, charts from the full mix) |
| **NVIDIA GPU (CUDA)** | Demucs in ~20 s per song instead of a few minutes | no (`--device cpu`) |

The easiest way to install yt-dlp and Demucs is [uv](https://docs.astral.sh/uv/):

```sh
# Linux / macOS
curl -LsSf https://astral.sh/uv/install.sh | sh
```

```powershell
# Windows (PowerShell)
winget install astral-sh.uv
```

### 1. itg-yt

**Prebuilt binary**: from the
[Releases](https://github.com/Alexis-benoist/itg-yt/releases) page (latest release = latest commit
on `main`), download the archive for your system:

| system | archive |
|---|---|
| Linux x86_64 | `itg-yt-vX.Y.Z-linux-x86_64.tar.gz` |
| macOS Apple Silicon (M1…) | `itg-yt-vX.Y.Z-macos-arm64.tar.gz` |
| macOS Intel | `itg-yt-vX.Y.Z-macos-x86_64.tar.gz` |
| Windows | `itg-yt-vX.Y.Z-windows-x86_64.zip` |

```sh
# Linux / macOS: verify, extract, put in the PATH
sha256sum -c itg-yt-vX.Y.Z-linux-x86_64.tar.gz.sha256   # macOS: shasum -a 256 -c …
tar xzf itg-yt-vX.Y.Z-linux-x86_64.tar.gz
install -m 755 itg-yt-vX.Y.Z-linux-x86_64/itg-yt ~/.local/bin/
```

On macOS the binary is not signed: before the first run,
`xattr -d com.apple.quarantine ~/.local/bin/itg-yt`. On Windows, extract the `.zip` and put
`itg-yt.exe` in a folder of the `PATH` (or run it from its folder).

**Or from source**: you need stable Rust ([rustup](https://rustup.rs)) and a C compiler, since
itg-charter compiles aubio:

| system | C compiler |
|---|---|
| Debian / Ubuntu | `sudo apt install build-essential` |
| Fedora | `sudo dnf install gcc` |
| macOS | `xcode-select --install` |
| Windows | [Build Tools for Visual Studio](https://visualstudio.microsoft.com/visual-cpp-build-tools/), "Desktop development with C++" workload |

```sh
git clone https://github.com/Alexis-benoist/itg-yt
cd itg-yt
cargo install --path .        # installs itg-yt into ~/.cargo/bin
```

Build **from the clone**: its `.cargo/config.toml` sets `CFLAGS=-D_DEFAULT_SOURCE`, without which
aubio's C code does not compile with GCC ≥ 14. (A `cargo install --git …` run elsewhere does not
read that file: export `CFLAGS` yourself in that case.)

### 2. ffmpeg

| system | command |
|---|---|
| Debian / Ubuntu | `sudo apt install ffmpeg` |
| Fedora | `sudo dnf install ffmpeg` (RPM Fusion repository; the stock `ffmpeg-free` lacks libx264) |
| Arch | `sudo pacman -S ffmpeg` |
| macOS | `brew install ffmpeg` |
| Windows | `winget install Gyan.FFmpeg` (full build) |

Check that both encoders are there:

```sh
ffmpeg -hide_banner -encoders | grep -E "libvorbis|libx264"      # Windows: findstr instead of grep
```

### 3. yt-dlp and a JavaScript runtime

```sh
uv tool install "yt-dlp[default]"     # [default] brings the EJS scripts
```

Then a JavaScript runtime, which yt-dlp needs for YouTube:

| system | command |
|---|---|
| Linux | deno: `curl -fsSL https://deno.land/install.sh \| sh`; or node: `sudo apt install nodejs` |
| macOS | `brew install deno` (or `brew install node`) |
| Windows | `winget install DenoLand.Deno` (or `winget install OpenJS.NodeJS.LTS`) |

yt-dlp uses deno by default; when only node is installed, itg-yt passes `--js-runtimes node`.
itg-yt prefers `~/.local/bin/yt-dlp` (where uv installs it), then the one in the `PATH`.

YouTube changes its protections often: **update yt-dlp** at the first download failure:

```sh
uv tool upgrade yt-dlp
```

### 4. Demucs (optional, recommended)

Charts are better when the drums and bass are separated from the rest. Without Demucs, itg-yt still
works: it prints `warning: no stems (…); analysing the full mix only` and charts the full mix
(`--no-stems` does the same, without the warning).

**Linux** (NVIDIA GPU: torch with CUDA is installed by default) and **macOS**:

```sh
uv venv --python 3.11 ~/.local/share/itg-charter/demucs-venv
VIRTUAL_ENV=~/.local/share/itg-charter/demucs-venv uv pip install demucs torch torchaudio soundfile
```

This is where itg-yt looks for Demucs by default. Without an NVIDIA GPU (which includes every Mac),
pass `--device cpu`: it works, but several times slower.

**Windows** (PowerShell):

```powershell
uv venv --python 3.11 $HOME\itg-charter\demucs-venv
$env:VIRTUAL_ENV = "$HOME\itg-charter\demucs-venv"
# NVIDIA GPU: torch with CUDA; without a GPU, drop the --index-url line and use --device cpu
uv pip install torch torchaudio --index-url https://download.pytorch.org/whl/cu124
uv pip install demucs soundfile
setx ITG_CHARTER_PYTHON "$HOME\itg-charter\demucs-venv\Scripts\python.exe"
```

For another Python where Demucs is already installed: `ITG_CHARTER_PYTHON=/path/to/python`.

The first song downloads the `htdemucs` model weights (≈ 80 MB). The separated tracks are then
cached (`~/.cache/itg-charter/stems/`): running a song again does not rerun Demucs.

### 5. Hook the output folder into the game (once)

itg-yt writes to `~/ITG-YouTube/` (`%USERPROFILE%\ITG-YouTube` on Windows). For the game to see
it, link it into ITGmania's `Songs` folder, where it shows up as the "YouTube" pack. The `Songs`
folder is:

| system | portable install | regular install |
|---|---|---|
| Linux | `<ITGmania folder>/Songs` | `~/.itgmania/Songs` |
| macOS | `<ITGmania folder>/Songs` | `~/Library/Application Support/ITGmania/Songs` |
| Windows | `<ITGmania folder>\Songs` | `%APPDATA%\ITGmania\Songs` |

```sh
# Linux / macOS (symbolic link; adjust the Songs path)
mkdir -p ~/ITG-YouTube
ln -sfn ~/ITG-YouTube ~/.itgmania/Songs/YouTube
```

```bat
:: Windows (cmd, junction: no administrator rights needed; adjust the Songs path)
mkdir %USERPROFILE%\ITG-YouTube
mklink /J "%APPDATA%\ITGmania\Songs\YouTube" "%USERPROFILE%\ITG-YouTube"
```

Alternatively, on any system, write straight into the game with `-o <Songs>/YouTube`.

### Check the installation

```sh
itg-yt --help
yt-dlp --version
itg-yt --no-video "https://www.youtube.com/watch?v=..."   # quick first try, without the video
```

The last line printed is the path of the created `.sm`. Restart ITGmania (or reload songs): the
song is in the "YouTube" pack.

## Usage

A video URL with `&list=…` (played from a mix or a playlist) gives only that video. A playlist URL
gives all its videos.

The folder `~/ITG-YouTube/<Title>/` contains:

| file | content |
|---|---|
| `<Title>.sm` | the charts, generated by [itg-charter](https://github.com/Alexis-benoist/itg-charter) (by default 5 charts of meter 2, 4, 6, 8 and 10) |
| `<Title>.ogg` | the audio as OGG Vorbis q5 (≈160 kb/s, transparent, 3 to 4 MB) |
| `<Title>-bg.mp4` | the music video as background movie, H.264 up to 1080p, 30 fps, no sound |
| `bn.png`, `bg.png`, `jacket.png` | 418×164 banner, 1920×1080 background and 512×512 jacket made from the thumbnail |

## Options

Charts:
- `-p beginner`: meters 2, 3, 4, 5; `-p full` (default): meters 2, 4, 6, 8, 10;
- `-m 2-5` or `-m 1,3,6`: chosen meters on the ITGmania 1-10 scale (at most 5);
- `-d easy,hard`: instead of meters, difficulty slots (beginner, easy, medium, hard, challenge or
  `all`) with the typical density of human charts of each slot;
- `-s 42`: seed (default 0; same video + same seed ⇒ same charts);
- `--no-stems` (no Demucs, faster), `--device cpu`.

General: `-o DIR`, `--no-video`, `--title`, `--artist` (single video only), `--cache DIR`.

Several songs:
- `--jobs 2` (default): songs downloaded and encoded at the same time. Charts (Demucs on the GPU)
  are made one song at a time, while the next songs are being prepared;
- a failed song does not stop the others; a summary is printed at the end, the paths of the
  successful `.sm` files are written to standard output, and the exit code is 1 if anything failed.

Reliability: yt-dlp retries network errors itself (`--retries`, `--fragment-retries`,
`--extractor-retries`). If a call still fails (crash, failed extraction), itg-yt reruns it up to
`--yt-retries 3` times, after 2 s, 4 s then 8 s. For the metadata, the grouped call is redone and
the results are merged.

Background video:
- `--video-height 720`: maximum height (default 1080; never upscaled);
- `--video-preset veryfast`: x264 speed (default `medium`);
- `--video-crf 26`: quality (lower = better and bigger).

Measured on Usher's "Yeah!" (4 min 10 s, 12 cores, GTX 1650, default settings): charts ready in
1 min 40 s, complete folder in 5 min 20 s; OGG 4.7 MB; 1080p video 115 MB. The 1080p encode is the
longest step: for speed, `--video-height 720 --video-preset veryfast`.

Environment variables: `ITG_YT_DLP` and `ITG_FFMPEG` (another yt-dlp / ffmpeg),
`ITG_CHARTER_PYTHON` (Python with Demucs).

## How it works

1. Metadata of **all URLs in a single** `yt-dlp -j` call (YouTube challenges are solved only once;
   duplicates removed). Title and artist come from YouTube's music fields (`track`, `artists`) when
   present, otherwise from the cleaned-up title ("Artist - Title (Official Video) [4K]" →
   "Title" / "Artist").
2. Audio, video (≤ 1080p) and thumbnail are downloaded **in parallel** into a cache
   (`~/.cache/itg-charter/youtube/<id>/`): running again downloads nothing.
3. ffmpeg encodes **in parallel** (audio, images, and the video at low priority), also cached:
   running again (another seed, other difficulties) re-encodes nothing, and changing the video
   quality only re-encodes the video.
4. As soon as the OGG and the images are ready, itg-charter (built into itg-yt) creates the folder
   and generates the charts from the OGG, the very file the game will play, while the video is
   still encoding.
5. itg-charter then adds the background video to the `.sm` (`#BGCHANGES`, timed to start with the
   audio).

## Troubleshooting

| symptom | fix |
|---|---|
| `warning: no stems (Demucs python not found at …)` | not blocking (charts from the full mix); to get stems: step 4 or `ITG_CHARTER_PYTHON`; to silence it: `--no-stems` |
| yt-dlp error: `Sign in to confirm…`, `n challenge`, `Requested format is not available` | `uv tool upgrade yt-dlp`, check it was installed with `[default]` and that deno or node is in the `PATH` |
| `Unknown encoder 'libx264'` or `'libvorbis'` | incomplete ffmpeg: get a full build (step 2) |
| `CUDA out of memory` | `--device cpu` (or close whatever is using the GPU) |
| build: `implicit declaration of function 'strncasecmp'` | build from the clone (see `.cargo/config.toml`) or `export CFLAGS=-D_DEFAULT_SOURCE` |
| the song does not show up in the game | check the link from step 5, then reload songs or restart ITGmania |

## Releases

Every push to `main` automatically publishes a GitHub release `vX.Y.<build number>` with binaries
for Linux x86_64, macOS (arm64, x86_64) and Windows x86_64 (when they build), as `.tar.gz`/`.zip`
+ sha256. Pushing a `vX.Y.Z` tag also publishes a release under that name.

## License

GPL-3.0, like itg-charter.
