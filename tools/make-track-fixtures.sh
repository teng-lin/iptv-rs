#!/usr/bin/env bash
# Synthetic streams with several tracks, for the player's track choosers
# (ffmpeg test patterns and tones, no real content), into fixtures/tracks/:
#   hls/master.m3u8  HLS: two video renditions (360p, 720p), two audio
#                    renditions (English stereo, French 5.1) and two WebVTT
#                    subtitle renditions (English, German)
#   multi.mkv        Matroska: one video, two audio tracks (English,
#                    Spanish), two subtitle tracks (English, French)
#   logo.png         a channel logo
#   tracks.m3u       a playlist of both, addressed at --host
# Usage: tools/make-track-fixtures.sh [--host 127.0.0.1:8097] [--serve]
# (the Android emulator reaches this machine at 10.0.2.2).
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
dir="$root/fixtures/tracks"
host=127.0.0.1:8097
serve=no
while [ $# -gt 0 ]; do
  case "$1" in
    --host) host=$2; shift 2 ;;
    --serve) serve=yes; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
mkdir -p "$dir/hls"
secs=120

srt() { # file, words: a cue every 4 s
  local out=$1 word=$2 i
  : > "$out"
  for ((i = 0; i < secs / 4; i++)); do
    printf '%d\n00:%02d:%02d,000 --> 00:%02d:%02d,500\n%s %d\n\n' \
      $((i + 1)) $((i * 4 / 60)) $((i * 4 % 60)) $((i * 4 / 60)) $((i * 4 % 60 + 3)) \
      "$word" $((i + 1)) >> "$out"
  done
}
srt "$dir/en.srt" "Synthetic subtitle"
srt "$dir/de.srt" "Synthetischer Untertitel"
srt "$dir/fr.srt" "Sous-titre synthétique"

hls() { # name, ffmpeg input and codec arguments: one HLS media playlist
  local name=$1
  shift
  ffmpeg -loglevel error -y "$@" -t $secs \
    -f hls -hls_time 4 -hls_playlist_type vod \
    -hls_segment_filename "$dir/hls/${name}_%03d.ts" "$dir/hls/$name.m3u8"
}

vtt() { # name, srt: one WebVTT segment, timed against the TS streams
  ffmpeg -loglevel error -y -i "$2" "$dir/hls/$1.vtt"
  # ffmpeg's TS streams start at 1.4 s (126000 at 90 kHz).
  sed -i '1a X-TIMESTAMP-MAP=MPEGTS:126000,LOCAL:00:00:00.000' "$dir/hls/$1.vtt"
  printf '#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:%d\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXTINF:%d.0,\n%s.vtt\n#EXT-X-ENDLIST\n' \
    $secs $secs "$1" > "$dir/hls/$1.m3u8"
}

if [ ! -f "$dir/hls/master.m3u8" ]; then
  video="testsrc2=size=1280x720:rate=25"
  x264=(-c:v libx264 -preset veryfast -g 50 -pix_fmt yuv420p -an)
  hls v360 -f lavfi -i "$video" -vf scale=640:360 "${x264[@]}" -b:v 600k
  hls v720 -f lavfi -i "$video" "${x264[@]}" -b:v 1800k
  hls a_en -f lavfi -i "sine=frequency=440:sample_rate=48000" -vn -c:a aac -b:a 96k -ac 2
  hls a_fr -f lavfi -i "sine=frequency=660:sample_rate=48000" -vn -c:a aac -b:a 192k -ac 6
  vtt t_en "$dir/en.srt"
  vtt t_de "$dir/de.srt"
  {
    echo '#EXTM3U'
    echo '#EXT-X-VERSION:4'
    echo '#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud",LANGUAGE="en",NAME="English",DEFAULT=YES,AUTOSELECT=YES,CHANNELS="2",URI="a_en.m3u8"'
    echo '#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud",LANGUAGE="fr",NAME="French",DEFAULT=NO,AUTOSELECT=YES,CHANNELS="6",URI="a_fr.m3u8"'
    echo '#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID="subs",LANGUAGE="en",NAME="English",DEFAULT=NO,AUTOSELECT=YES,URI="t_en.m3u8"'
    echo '#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID="subs",LANGUAGE="de",NAME="German",DEFAULT=NO,AUTOSELECT=YES,URI="t_de.m3u8"'
    echo '#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,FRAME-RATE=25.000,AUDIO="aud",SUBTITLES="subs"'
    echo 'v360.m3u8'
    echo '#EXT-X-STREAM-INF:BANDWIDTH=2200000,RESOLUTION=1280x720,FRAME-RATE=25.000,AUDIO="aud",SUBTITLES="subs"'
    echo 'v720.m3u8'
  } > "$dir/hls/master.m3u8"
  echo "made hls/master.m3u8"
fi

if [ ! -f "$dir/multi.mkv" ]; then
  ffmpeg -loglevel error -y \
    -f lavfi -i "smptebars=size=1280x720:rate=25" \
    -f lavfi -i "sine=frequency=330:sample_rate=48000" \
    -f lavfi -i "sine=frequency=880:sample_rate=48000" \
    -i "$dir/en.srt" -i "$dir/fr.srt" \
    -t $secs -map 0:v -map 1:a -map 2:a -map 3:s -map 4:s \
    -c:v libx264 -preset veryfast -g 50 -pix_fmt yuv420p \
    -c:a aac -b:a 96k -ac 2 -c:s srt \
    -metadata:s:a:0 language=eng -metadata:s:a:1 language=spa \
    -metadata:s:s:0 language=eng -metadata:s:s:1 language=fre \
    "$dir/multi.mkv"
  echo "made multi.mkv"
fi

[ -f "$dir/logo.png" ] ||
  ffmpeg -loglevel error -y -f lavfi -i "color=c=0x2a7fbf:size=160x90,format=rgb24" \
    -frames:v 1 "$dir/logo.png"

{
  echo '#EXTM3U'
  echo "#EXTINF:-1 tvg-id=\"tracks.hls.example\" tvg-logo=\"http://$host/logo.png\" tvg-chno=\"1\" group-title=\"Track tests\",Synthetic HLS renditions"
  echo "http://$host/hls/master.m3u8"
  echo "#EXTINF:-1 tvg-id=\"tracks.mkv.example\" tvg-chno=\"2\" group-title=\"Track tests\",Synthetic Matroska tracks"
  echo "http://$host/multi.mkv"
} > "$dir/tracks.m3u"
echo "playlist: http://$host/tracks.m3u"

if [ "$serve" = yes ]; then
  exec python3 -m http.server "${host##*:}" --bind 0.0.0.0 --directory "$dir"
fi
