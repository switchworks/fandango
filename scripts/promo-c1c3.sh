#!/usr/bin/env bash
# 紹介動画の C1–C3（0.000–12.000）を通しで出す。確認用であり完成品ではない。
#
# 構成は plans/promo_video_v01.md §5.5 のとおり:
#   C1 0.000–4.000  地色 #171717 + tagline 英日（0.500–1.500 でフェードイン）
#   C2 4.000–10.000 crush の GUI が 0.600 秒で合焦、rate を等比に掃引。tagline 継続 → 10.000 で瞬断
#   C3 10.000–12.000 ワードマーク（案 B）。11.400–12.000 でフェードアウト
#
# 音は tmp/promo/ の 2 本を使う。C1-C2 側の末尾にはマイクロフェード（クリック回避）を掛ける。
#
# 使い方: scripts/promo-c1c3.sh [出力パス]
set -euo pipefail

FFMPEG=C:/ffmpeg/bin/ffmpeg
CARGO=${CARGO:-$HOME/.cargo/bin/cargo}
SHOTS=target/release/fandango-shots.exe
FONT='D\:/wrk/fandango/assets/fonts/WDXLLubrifontJPN-Regular.ttf'
BG=0x171717
FPS=60
OUT=${1:-tmp/promo/C1-C3.mp4}

# 音は通し wav 1 本（C1-C3.wav）を優先する。無ければカット別の 2 本から組む。
WAV_THRU="tmp/promo/C1-C3.wav"
WAV_A="tmp/promo/C1-C2_intro_crush.wav"
WAV_C="tmp/promo/C3_fandango by switchworks.wav"
if [ ! -f "$WAV_THRU" ]; then
  for f in "$WAV_A" "$WAV_C"; do
    [ -f "$f" ] || { echo "音素材が無い: $f（通し $WAV_THRU も無い）" >&2; exit 1; }
  done
fi
[ -f assets/fonts/WDXLLubrifontJPN-Regular.ttf ] || {
  echo "フォントが無い: assets/fonts/WDXLLubrifontJPN-Regular.ttf（licenses/NON-CARGO.md の手順で取得）" >&2
  exit 1
}

TAG_EN='Closed form. So it never breaks.'
TAG_JA='式で閉じる。だから、壊れない。'
# tagline（§4）: 英語 104px / 日本語 64px / 縁取り 12px / 画面中央
tagline() { # $1 = alpha 式
  echo "drawtext=fontfile='$FONT':text='$TAG_EN':fontsize=104:fontcolor=white:borderw=12:bordercolor=$BG:x=(w-tw)/2:y=(h-th)/2-70:alpha='$1',drawtext=fontfile='$FONT':text='$TAG_JA':fontsize=64:fontcolor=0xd8d8d8:borderw=12:bordercolor=$BG:x=(w-tw)/2:y=(h-th)/2+70:alpha='$1'"
}

work=$(mktemp -d -p tmp/promo c1c3.XXXX)
trap 'rm -rf "$work" "$work.mp4"' EXIT

"$CARGO" build -p fandango-shots --release --quiet

# ── C2 の GUI（6.000 秒）。crush は bits 8.00 / mix 100% 固定、rate を 1.00 → 32.00 へ ──
"$SHOTS" anim --crate crush --fps "$FPS" --scale 2.0 \
  --key "0:bits=0.4667,rate=0.0,mix=1.0" \
  --key "6:bits=0.4667,rate=1.0,mix=1.0" \
  --out "$work/c2" >/dev/null 2>&1

# 合焦: sigma 固定の gblur を 1 フレーム 1 段で並べ、enable で切り替える。
# （gblur の sigma は sendcmd で変えられない。実測 2026-08-28）
blur=""
steps=$(awk -v f="$FPS" 'BEGIN{printf "%d", 0.6*f + 0.5}')
for i in $(seq 0 $((steps-1))); do
  sg=$(awk -v i="$i" -v n="$steps" 'BEGIN{u=i/n; printf "%.3f", 24*(1-u)*(1-u)}')
  a=$(awk -v i="$i" -v n="$steps" 'BEGIN{printf "%.4f", 0.6*i/n}')
  b=$(awk -v i="$i" -v n="$steps" 'BEGIN{printf "%.4f", 0.6*(i+1)/n}')
  blur="$blur,gblur=sigma=$sg:enable='between(t,$a,$b)'"
done

enc=(-c:v libx264 -pix_fmt yuv420p -crf 18 -r "$FPS")

# C1: 地色 + tagline（0.500–1.500 でフェードイン、以降 1.0）
"$FFMPEG" -y -v error -f lavfi -i "color=$BG:s=1920x1080:r=$FPS" -t 4 \
  -vf "$(tagline 'if(lt(t,0.5),0,if(lt(t,1.5),(t-0.5),1))')" "${enc[@]}" "$work/c1.mp4"

# C2: GUI を pad → 合焦 → tagline を重ねる（tagline は blur より後に描く）
"$FFMPEG" -y -v error -framerate "$FPS" -i "$work/c2/%05d.png" \
  -vf "pad=1920:1080:160:140:color=$BG$blur,$(tagline 1)" \
  -frames:v $((FPS*6)) "${enc[@]}" "$work/c2.mp4"

# C3: ワードマーク（案 B）。11.400–12.000 でフェードアウト＝カット内 1.400–2.000
"$FFMPEG" -y -v error -f lavfi -i "color=$BG:s=1920x1080:r=$FPS" -t 2 \
  -vf "drawtext=fontfile='$FONT':text='fandango':fontsize=380:fontcolor=white:x=(w-tw)/2:y=(h-th)/2-90:alpha='if(lt(t,1.4),1,max(0,1-(t-1.4)/0.6))',drawtext=fontfile='$FONT':text='by switchworks':fontsize=110:fontcolor=0x9a9a9a:x=(w-tw)/2:y=(h/2)+150:alpha='if(lt(t,1.4),1,max(0,1-(t-1.4)/0.6))'" \
  "${enc[@]}" "$work/c3.mp4"

printf "file '%s'\n" c1.mp4 c2.mp4 c3.mp4 > "$work/list.txt"
"$FFMPEG" -y -v error -f concat -safe 0 -i "$work/list.txt" -c copy "$work/video.mp4"

# 音を貼る。通し wav があればそのまま使う（マイクロフェードもユーザー側で焼かれている前提）。
if [ -f "$WAV_THRU" ]; then
  "$FFMPEG" -y -v error -i "$work/video.mp4" -i "$WAV_THRU" -map 0:v -map 1:a -c:v copy -c:a aac -b:a 256k -t 12 "$OUT"
else
  # 分割素材から組む。C1-C2 の末尾に 20 ms のマイクロフェード（瞬断のクリック回避）を掛け、
  # C3 の声を 10.000 から重ねる。
  "$FFMPEG" -y -v error -i "$work/video.mp4" -i "$WAV_A" -i "$WAV_C" -filter_complex "[1:a]afade=t=out:st=9.98:d=0.02[a1];[2:a]adelay=10000|10000[a2];[a1][a2]amix=inputs=2:normalize=0,apad=whole_dur=12[a]" -map 0:v -map "[a]" -c:v copy -c:a aac -b:a 256k -t 12 "$OUT"
fi

C:/ffmpeg/bin/ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$OUT"
echo "$OUT"
