#!/usr/bin/env bash
# Build SwiftHarryDM's small FFmpeg for Windows x64 (cross-compiled with mingw-w64).
#
# Only what the app and yt-dlp use is enabled: reading MP4/WebM/MKV/MPEG-TS/audio files,
# joining video and audio without re-encoding, cleaning up HLS/DASH downloads, writing
# MP4/M4A/MKV/WebM/MP3, converting audio to MP3 (LAME) or AAC, and ffprobe for checks.
# No network, no hardware acceleration, no video decoding or scaling. LGPL (no GPL parts).
#
# Usage (Linux or Docker):  OUT=/out bash scripts/ffmpeg/build.sh
#   docker run --rm -v "$PWD/scripts/ffmpeg:/src:ro" -v "$PWD/tools/ffmpeg-slim:/out" ubuntu:24.04 bash /src/build.sh
set -euo pipefail

FFMPEG_VERSION=8.1.3
FFMPEG_SHA256=7138d28c96d9d3e3af4ee3d8cad72741f8ffb40da90c1112235dea3ecd3178a3
LAME_VERSION=3.100
LAME_SHA256=ddfe36cab873794038ae2c1210557ad34857a4b6bdc515785d1da9e175b1da1e

OUT=${OUT:-/out}
WORK=/tmp/build
DEPS=/tmp/deps
HOST=x86_64-w64-mingw32
JOBS=$(nproc)

if command -v apt-get >/dev/null && [ "$(id -u)" = 0 ]; then
  export DEBIAN_FRONTEND=noninteractive
  apt-get update -qq
  apt-get install -y -qq --no-install-recommends ca-certificates curl xz-utils make nasm pkg-config gcc libc6-dev \
    gcc-mingw-w64-x86-64-posix g++-mingw-w64-x86-64-posix binutils-mingw-w64-x86-64 >/dev/null
fi

mkdir -p "$WORK" "$DEPS" "$OUT"
cd "$WORK"

fetch() { # url sha256 file
  curl -fsSL --retry 3 -o "$3" "$1"
  local got
  got=$(sha256sum "$3" | cut -d' ' -f1)
  if [ "$2" = PIN ]; then
    echo "PIN $3 sha256=$got"
  elif [ "$got" != "$2" ]; then
    echo "SHA-256 mismatch for $3: got $got, expected $2" >&2
    exit 1
  fi
}

# ---------------------------------------------------------------- LAME (MP3 encoder, LGPL)
fetch "https://downloads.sourceforge.net/project/lame/lame/${LAME_VERSION}/lame-${LAME_VERSION}.tar.gz" "$LAME_SHA256" lame.tar.gz
tar xf lame.tar.gz
(
  cd "lame-${LAME_VERSION}"
  # LAME 3.100 exports a symbol that no longer exists; drop it from the export list.
  sed -i '/lame_init_old/d' include/libmp3lame.sym
  ./configure --host=$HOST --prefix="$DEPS" --enable-static --disable-shared --disable-frontend --disable-decoder \
    --disable-gtktest --enable-nasm CFLAGS="-O2" >/dev/null
  make -j"$JOBS" >/dev/null
  make install >/dev/null
)

# ---------------------------------------------------------------- FFmpeg
fetch "https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz" "$FFMPEG_SHA256" ffmpeg.tar.xz
tar xf ffmpeg.tar.xz
cd "ffmpeg-${FFMPEG_VERSION}"

CONFIGURE=(
  --prefix="$WORK/install" --target-os=mingw32 --arch=x86_64 --cross-prefix=$HOST-
  --enable-static --disable-shared --pkg-config=pkg-config --pkg-config-flags=--static
  --extra-cflags="-I$DEPS/include -O2" --extra-ldflags="-L$DEPS/lib -static"
  --disable-everything --disable-autodetect --disable-network --disable-doc --disable-debug
  --disable-ffplay --enable-ffmpeg --enable-ffprobe
  --disable-avdevice --disable-swscale
  --enable-libmp3lame
  --enable-protocol=file,pipe
  --enable-demuxer=mov,matroska,mpegts,aac,mp3,ogg,flac,wav,ac3,eac3,h264,hevc,m4v,webvtt,srt
  --enable-muxer=mp4,mov,ipod,matroska,webm,mp3,mpegts,adts,ogg,opus,flac,wav,null,webvtt,srt
  --enable-parser=h264,hevc,aac,aac_latm,av1,vp8,vp9,opus,vorbis,mpegaudio,flac,ac3
  --enable-bsf=aac_adtstoasc,h264_mp4toannexb,hevc_mp4toannexb,extract_extradata,vp9_superframe,vp9_superframe_split,av1_frame_merge,av1_frame_split,null,setts
  --enable-decoder=aac,aac_latm,mp3,mp3float,opus,vorbis,flac,ac3,eac3,pcm_s16le,pcm_s24le,pcm_f32le,webvtt,subrip
  --enable-encoder=libmp3lame,aac,flac,pcm_s16le,webvtt,subrip,mov_text
  --enable-filter=aresample,aformat,anull,null,format,abuffer,abuffersink,buffer,buffersink,atrim,trim,asetpts,setpts,volume
)
if ! PKG_CONFIG_PATH="$DEPS/lib/pkgconfig" ./configure "${CONFIGURE[@]}" > configure.out 2>&1; then
  tail -40 configure.out; tail -60 ffbuild/config.log; exit 1
fi
make -j"$JOBS" > make.out 2>&1 || { grep -m 20 -B 2 -A 8 "error" make.out; exit 1; }
make install >/dev/null

for exe in ffmpeg ffprobe; do
  $HOST-strip -o "$OUT/$exe.exe" "$WORK/install/bin/$exe.exe"
done
cp COPYING.LGPLv2.1 "$OUT/FFMPEG-LICENSE.txt"
cp "$WORK/lame-${LAME_VERSION}/COPYING" "$OUT/LAME-LICENSE.txt"
{
  echo "SwiftHarryDM FFmpeg build"
  echo "FFmpeg ${FFMPEG_VERSION} (https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz)"
  echo "LAME ${LAME_VERSION} (https://sourceforge.net/projects/lame/)"
  echo "Built with scripts/ffmpeg/build.sh. Configure options:"
  printf '  %s\n' "${CONFIGURE[@]}"
} > "$OUT/BUILD-INFO.txt"
ls -la "$OUT"
