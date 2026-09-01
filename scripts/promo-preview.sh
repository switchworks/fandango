#!/usr/bin/env bash
# 紹介動画のカット単位プレビュー（映像 + 音）を 1 コマンドで出す。
#
# 用途: `plans/promo_video_v01.md` の掃引値を詰める反復。GUI 映像を shots で出し、
# 1920x1080 へ pad し、用意済みの wav を mux する。完成品ではなく確認用。
#
# 使い方:
#   scripts/promo-preview.sh --crate crush --wav tmp/promo/C1-C2_intro_crush.wav \
#     --out tmp/promo/C2.mp4 \
#     --key 0:bits=1.0,rate=0.0,mix=0.0 --key 10:bits=0.2,rate=0.5,mix=1.0
#
# --key は shots anim と同一書式（時刻は秒、値は正規化 0.0-1.0）。
# 尺は最後の --key の時刻で決まる。カット尺ちょうどの位置に終端キーを置くこと。
#
# キー間の補間は線形（crates/shots/src/anim.rs:86）。曲線の掃引はキーを細かく
# 打って近似する（0.5 秒刻み程度で足りる）。DAW 側を直線に制限する必要はない。
#
# 生成コスト実測 (2026-08-28, crush・単体 GUI・scale 2.0): 0.045 秒/フレーム。
# 60 fps / 10 秒のカットで 26.7 秒。詰めの段階でも 60 fps で回してよい。
# （タイル動画の 4 秒/フレームは 12 crate を描き直すため。単体 GUI とは 90 倍違う）
set -euo pipefail

FFMPEG=C:/ffmpeg/bin/ffmpeg
CARGO=${CARGO:-$HOME/.cargo/bin/cargo}
SHOTS=target/release/fandango-shots.exe
BG=0x171717

crate=""; wav=""; out=""; fps=60; scale=2.0; fade_out=""; in_fade=""; in_blur=""; keys=()
while [ $# -gt 0 ]; do
  case "$1" in
    --crate) crate=$2; shift 2 ;;
    --wav)   wav=$2; shift 2 ;;
    --out)   out=$2; shift 2 ;;
    --fps)   fps=$2; shift 2 ;;
    --scale) scale=$2; shift 2 ;;
    --key)   keys+=(--key "$2"); shift 2 ;;
    # 音のフェードアウト。書式 <開始秒>:<長さ秒>[:<curve>]（curve 既定 tri = 線形）
    --fade-out) fade_out=$2; shift 2 ;;
    # 映像の頭を地色から明度で立ち上げる。書式 <長さ秒>
    --in-fade) in_fade=$2; shift 2 ;;
    # 映像の頭をぼかしから合焦させる。書式 <長さ秒>[:<開始sigma>]（sigma 既定 24）
    --in-blur) in_blur=$2; shift 2 ;;
    *) echo "unknown flag: $1" >&2; exit 2 ;;
  esac
done
[ -n "$crate" ] && [ -n "$out" ] && [ ${#keys[@]} -gt 0 ] || {
  echo "usage: $0 --crate <name> --out <path.mp4> --key <t>:<id>=<v>[,…] … [--wav <f.wav>] [--fps n] [--scale f] [--fade-out st:d[:curve]]" >&2
  exit 2
}

# shots のフレーム数は duration*fps + 1（crates/shots/src/anim.rs:105）。
# そのまま繋ぐとカットごとに 1 フレーム長くなり concat で累積するので、末尾を捨てる。
dur=$(printf '%s
' "${keys[@]}" | grep -v '^--key$' | cut -d: -f1 | sort -g | tail -1)
nframes=$(awk -v d="$dur" -v f="$fps" 'BEGIN{printf "%d", d*f + 0.5}')

"$CARGO" build -p fandango-shots --release --quiet
frames=$(mktemp -d -p tmp/promo frames.XXXX)
trap 'rm -rf "$frames" "${frames%.*}.mp4"' EXIT

"$SHOTS" anim --crate "$crate" --fps "$fps" --scale "$scale" "${keys[@]}" --out "$frames" >/dev/null 2>&1

# GUI 1600x800 を 1920x1080 の中央へ（型 A の余白 160/140、地は #171717）
vf="pad=1920:1080:160:140:color=$BG"

# 頭の立ち上がり。--in-fade は地色からの明度フェード（1 フィルタで滑らか）。
# --in-blur は gblur の sigma を sendcmd で毎フレーム更新する（合焦）。
if [ -n "$in_fade" ]; then
  vf="$vf,fade=t=in:st=0:d=$in_fade:color=$BG"
fi
if [ -n "$in_blur" ]; then
  bd=${in_blur%%:*}; bs=24
  [ "$in_blur" != "$bd" ] && bs=${in_blur#*:}
  # gblur の sigma は sendcmd で変えられない（コマンドは受理されるが描画に反映されない。
  # 実測 2026-08-28: 全編ぼけたまま、最終フレームの PSNR が基準画に対し 23.6 dB）。
  # 代わりに sigma 固定の gblur を段数だけ並べ、timeline の enable で 1 段ずつ効かせる。
  # 無効な段は素通しなので費用は段数に比例しない。
  # 段数はフレーム数に一致させる（1 フレーム 1 段）。20 段固定だと 0.6 秒/60 fps で
  # 1 段 1.8 フレームとなり段階が見える（実測 2026-08-28）。
  steps=$(awk -v d="$bd" -v f="$fps" 'BEGIN{printf "%d", d*f + 0.5}')
  for i in $(seq 0 $((steps-1))); do
    # sigma は 2 乗で落とす。線形だと効きの大きい低 sigma 域が一瞬で過ぎ、
    # 合焦が終盤に集中する（実測: 線形では t=0.45→0.62 で PSNR 24→40 dB）。
    sg=$(awk -v i="$i" -v n="$steps" -v s="$bs" 'BEGIN{u=i/n; printf "%.3f", s*(1-u)*(1-u)}')
    a=$(awk -v i="$i" -v n="$steps" -v d="$bd" 'BEGIN{printf "%.4f", d*i/n}')
    b=$(awk -v i="$i" -v n="$steps" -v d="$bd" 'BEGIN{printf "%.4f", d*(i+1)/n}')
    vf="$vf,gblur=sigma=$sg:enable='between(t,$a,$b)'"
  done
fi

if [ -n "$wav" ]; then
  af=""
  if [ -n "$fade_out" ]; then
    st=${fade_out%%:*}; rest=${fade_out#*:}
    d=${rest%%:*}; curve=tri
    [ "$rest" != "$d" ] && curve=${rest#*:}
    af="-af afade=t=out:st=$st:d=$d:curve=$curve"
  fi
  # shellcheck disable=SC2086
  "$FFMPEG" -y -loglevel error -framerate "$fps" -i "$frames/%05d.png" -i "$wav" \
    -vf "$vf" -c:v libx264 -pix_fmt yuv420p -crf 18 \
    $af -c:a aac -b:a 256k -shortest "$out"
else
  "$FFMPEG" -y -loglevel error -framerate "$fps" -i "$frames/%05d.png" \
    -vf "$vf" -frames:v "$nframes" -c:v libx264 -pix_fmt yuv420p -crf 18 "$out"
fi

"C:/ffmpeg/bin/ffprobe" -v error -show_entries format=duration -of default=nw=1:nk=1 "$out"
echo "$out"
