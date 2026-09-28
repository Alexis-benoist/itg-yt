#!/usr/bin/env bash
# Newcomer scenarios on Linux / macOS, with an empty environment: a fresh HOME and a PATH
# with only the system tools `itg-yt setup` needs (no yt-dlp, deno, node or ffmpeg).
#
#   ci/newbie.sh <itg-yt binary>            # scenarios 1-5 (no YouTube access needed)
#   ci/newbie.sh <itg-yt binary> youtube    # real YouTube download, after the above
#
# Work folder: $NEWBIE_WORK (default: $RUNNER_TEMP/newbie or a temp folder); the youtube
# mode reuses the tools installed there by the first mode.
set -euo pipefail

BIN=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
MODE=${2:-all}
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=${NEWBIE_WORK:-${RUNNER_TEMP:-$(mktemp -d)}/newbie}
URL="https://www.youtube.com/watch?v=abc123"

fail() {
    echo "FAIL: $*" >&2
    exit 1
}
step() { echo; echo "=== $*"; }

# $1 must contain $2 (fixed string).
contains() { grep -qF -- "$2" <<<"$1" || fail "expected '$2' in: $1"; }
lacks() { ! grep -qF -- "$2" <<<"$1" || fail "unexpected '$2' in: $1"; }

HOME_DIR=$WORK/home
SYS=$WORK/sysbin
if [ "$(uname)" = Darwin ]; then
    TOOLS="$HOME_DIR/Library/Application Support/itg-yt/bin"
else
    TOOLS="$HOME_DIR/.local/share/itg-yt/bin"
fi

# Runs itg-yt in the empty environment; sets OUT, ERR and CODE.
itg() {
    local o e
    o=$(mktemp)
    e=$(mktemp)
    set +e
    env -i HOME="$HOME_DIR" PATH="$SYS" ITG_YT_RETRY_PAUSE_MS=10 ${EXTRA_ENV[@]+"${EXTRA_ENV[@]}"} \
        "$BIN" "$@" >"$o" 2>"$e"
    CODE=$?
    set -e
    OUT=$(cat "$o")
    ERR=$(cat "$e")
    rm -f "$o" "$e"
    echo "itg-yt $* -> exit $CODE"
}
EXTRA_ENV=()

if [ "$MODE" = youtube ]; then
    step "real YouTube download with the tools installed by setup"
    itg --no-video --no-stems -m 3 "https://www.youtube.com/watch?v=jNQXAC9IVRw"
    echo "$ERR"
    [ "$CODE" = 0 ] || fail "real download failed"
    [ -f "$OUT" ] || fail "no .sm: $OUT"
    echo "OK: $OUT"
    exit 0
fi

rm -rf "$WORK"
mkdir -p "$HOME_DIR" "$SYS" "$WORK/Songs"
# The system tools setup and the fake yt-dlp use, and nothing else.
for t in sh curl tar xz unzip nice cat cp rm sed grep mkdir; do
    p=$(command -v "$t" || true)
    [ -n "$p" ] && ln -s "$p" "$SYS/$t"
done
for t in yt-dlp deno node ffmpeg; do
    ! env -i PATH="$SYS" sh -c "command -v $t" >/dev/null ||
        fail "$t is visible in the test PATH"
done
echo "PATH for itg-yt: $(ls "$SYS" | tr '\n' ' ')"

step "1. nothing installed: exit code 2 before any download"
itg --no-stems "$URL"
echo "$ERR"
[ "$CODE" = 2 ] || fail "exit code $CODE, expected 2"
contains "$ERR" "itg-yt setup"
contains "$ERR" "yt-dlp (downloads the videos from YouTube): not found"
contains "$ERR" "deno or node"
contains "$ERR" "ffmpeg (encodes the audio with libvorbis and the background video with libx264)"
[ ! -e "$HOME_DIR/.cache/itg-charter/youtube" ] || fail "something was downloaded"

step "2. --no-video: libx264 is not required"
itg --no-video --no-stems "$URL"
[ "$CODE" = 2 ] || fail "exit code $CODE, expected 2"
contains "$ERR" "ffmpeg (encodes the audio with libvorbis)"
lacks "$ERR" "libx264"

step "3. itg-yt setup"
itg setup --songs "$WORK/Songs"
echo "$OUT"
[ "$CODE" = 0 ] || fail "setup: exit code $CODE: $ERR"
for t in yt-dlp deno ffmpeg ffprobe; do
    [ -x "$TOOLS/$t" ] || fail "$TOOLS/$t not installed"
done
[ -L "$WORK/Songs/YouTube" ] || fail "Songs/YouTube is not a symbolic link"
[ "$(readlink "$WORK/Songs/YouTube")" = "$HOME_DIR/ITG-YouTube" ] ||
    fail "Songs/YouTube -> $(readlink "$WORK/Songs/YouTube")"
[ -d "$HOME_DIR/ITG-YouTube" ] || fail "output folder not created"

step "3b. setup again: nothing downloaded"
touch "$WORK/marker"
itg setup --songs "$WORK/Songs"
[ "$CODE" = 0 ] || fail "second setup: exit code $CODE"
lacks "$OUT" "downloading"
[ -z "$(find "$TOOLS" -newer "$WORK/marker")" ] || fail "tools changed by the second setup"

step "3c. an existing YouTube folder is not replaced"
mkdir -p "$WORK/Songs2/YouTube"
echo keep >"$WORK/Songs2/YouTube/keep.txt"
itg setup --songs "$WORK/Songs2"
echo "$OUT"
[ "$CODE" = 1 ] || fail "exit code $CODE, expected 1"
contains "$OUT" "left untouched"
[ ! -L "$WORK/Songs2/YouTube" ] && [ "$(cat "$WORK/Songs2/YouTube/keep.txt")" = keep ] ||
    fail "existing folder modified"

step "4. setup --check: everything OK"
itg setup --check --songs "$WORK/Songs"
echo "$OUT"
[ "$CODE" = 0 ] || fail "check: exit code $CODE"
contains "$OUT" "ok       yt-dlp"
contains "$OUT" "ok       deno or node"
contains "$OUT" "ok       ffmpeg"
contains "$OUT" "ok       $WORK/Songs/YouTube"

step "5. full run with the downloaded ffmpeg and a fake yt-dlp (no YouTube)"
SRC=$WORK/src
mkdir -p "$SRC"
FF="$TOOLS/ffmpeg"
"$FF" -v error -f lavfi -i "aevalsrc=sin(2*PI*60*t)*exp(-30*mod(t\,0.5)):s=44100:d=12" \
    -ac 2 -c:a libvorbis "$SRC/audio.ogg"
"$FF" -v error -f lavfi -i testsrc2=size=320x180:rate=25:duration=2 -pix_fmt yuv420p \
    -c:v libx264 "$SRC/video.mp4"
"$FF" -v error -f lavfi -i testsrc=size=1280x720 -frames:v 1 "$SRC/thumb.jpg"
EXTRA_ENV=(ITG_YT_DLP="$HERE/fake-yt-dlp" FAKE_YT_SRC="$SRC")
itg --no-stems -m 3 --video-height 144 --video-preset ultrafast "$URL"
echo "$ERR"
[ "$CODE" = 0 ] || fail "run failed"
SONG="$WORK/Songs/YouTube/Great Song"
for f in "Great Song.sm" "Great Song.ogg" "Great Song-bg.mp4" bn.png bg.png jacket.png; do
    [ -s "$SONG/$f" ] || fail "$SONG/$f missing"
done
[ "$OUT" = "$HOME_DIR/ITG-YouTube/Great Song/Great Song.sm" ] || fail "stdout: $OUT"
"$TOOLS/ffprobe" -v error -show_entries stream=codec_name -of csv=p=0 "$SONG/Great Song.ogg" |
    grep -qx vorbis || fail "OGG is not Vorbis"
"$TOOLS/ffprobe" -v error -show_entries stream=codec_name -of csv=p=0 "$SONG/Great Song-bg.mp4" |
    grep -qx h264 || fail "video is not H.264"
grep -q "^#BGCHANGES:.*Great Song-bg.mp4" "$SONG/Great Song.sm" || fail "no #BGCHANGES"

echo
echo "All newcomer scenarios passed."
