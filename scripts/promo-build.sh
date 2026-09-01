#!/usr/bin/env bash
# 紹介動画を通しでビルドする。plans/promo_video_v01.md §5 のカット表が正本。
#
#   C1  0.000–4.000     地色 + tagline
#   C2  4.000–10.000    crush（合焦 0.6 秒 → rate 掃引）
#   C3  10.000–12.000   ワードマーク
#   C5  12.000–28.000   nice-bulk
#   C6  28.000–44.000   nimbus
#   C7  44.000–60.000   raindrop
#   C8  60.000–88.000   モンタージュ 7 本（各 4 秒）
#   C9  88.000–108.000  チェーン 4 段（2×2）
#   C4  108.000–116.000 12 本タイル
#   C10 116.000–128.000 エンド
#
# 音は tmp/promo/promo.wav（楽曲の実尺 126.000 秒）。動画の 128.000 秒との差 2 秒は
# ここで無音をパディングする（§5.6）。
#
# 使い方:
#   scripts/promo-build.sh [出力パス] [fps]
#   PROMO_ONLY=C5,C9 scripts/promo-build.sh   # 指定カットだけ作り直す（他は中間物を再利用）
set -euo pipefail

FFMPEG=C:/ffmpeg/bin/ffmpeg
FFPROBE=C:/ffmpeg/bin/ffprobe
CARGO=${CARGO:-$HOME/.cargo/bin/cargo}
SHOTS=target/release/fandango-shots.exe
FONT='D\:/wrk/fandango/assets/fonts/WDXLLubrifontJPN-Regular.ttf'
BG=0x171717
WAV=tmp/promo/promo.wav
WORK=tmp/promo/build
OUT=${1:-tmp/promo/promo.mp4}
FPS=${2:-60}

[ -f "$WAV" ] || { echo "音が無い: $WAV" >&2; exit 1; }
[ -f assets/fonts/WDXLLubrifontJPN-Regular.ttf ] || {
  echo "フォントが無い: assets/fonts/WDXLLubrifontJPN-Regular.ttf（licenses/NON-CARGO.md）" >&2
  exit 1
}
mkdir -p "$WORK"
"$CARGO" build -p fandango-shots --release --quiet

only=${PROMO_ONLY:-}
want() { [ -z "$only" ] || [[ ",$only," == *",$1,"* ]]; }

enc=(-c:v libx264 -pix_fmt yuv420p -crf 18 -r "$FPS")
pad="pad=1920:1080:160:140:color=$BG"

TAG_EN="Closed form. So it never breaks."
TAG_JA="式で閉じる。だから、壊れない。"

# 文字はファイル経由で渡す。drawtext の text= に直接書くと、アポストロフィ・コロン・カンマの
# エスケープがシェルと ffmpeg で二重になり事故る（実際 "It doesn't widen." の
# アポストロフィが落ちた。2026-08-31）。
TXT="$WORK/txt"
mkdir -p "$TXT"
txt() { # $1=識別子 $2=本文 → textfile へ渡すパスを返す
  printf '%s' "$2" >"$TXT/$1.txt"
  echo "$TXT/$1.txt"
}

# 文字幅は実測して決める。`awk` の `length()` は**バイト数**を返すため
# （実測 2026-08-31: 13 文字の日本語が 39）、文字数で縮小率を決めると日本語だけが
# 3 倍の勢いで縮み、漢字とかなの比率で大きさがブレる。実際 C8 の日本語がそうなっていた。
#
# 実測すると 12 本の line で 1600px を超えるのは drawl の英語だけ（2171px）。
# 日本語は最大でも 1016px（@54）／1656px（@88）で、縮小は要らなかった。
TEXT_MAX_W=1600

# `$1` を `$2` px で描いたときの実幅 [px] を返す。
text_width() {
  printf '%s' "$1" >"$TXT/_measure.txt"
  "$FFMPEG" -y -v error -f lavfi -i "color=black:s=4000x400" -frames:v 1     -vf "drawtext=fontfile='$FONT':textfile='$TXT/_measure.txt':fontsize=$2:fontcolor=white:x=20:y=100"     "$TXT/_measure.png"
  python -c "
from PIL import Image
b = Image.open('$TXT/_measure.png').convert('L').getbbox()
print(b[2] - b[0] if b else 0)"
}

# `$1` を `$2` px 基準で描き、`TEXT_MAX_W` に収まる最大サイズを返す。
fit_width() {
  local w size=$2
  w=$(text_width "$1" "$size")
  while [ "$w" -gt "$TEXT_MAX_W" ] && [ "$size" -gt 24 ]; do
    size=$((size - 4))
    w=$(text_width "$1" "$size")
  done
  echo "$size"
}

# 型 A のコピー（英語 88px 上帯 / 日本語 54px 下帯、縁取り 4px。§4）
copy_a() { # $1=識別子 $2=en $3=ja $4=alpha式
  local fe fj se sj
  fe=$(txt "${1}_en" "$2")
  fj=$(txt "${1}_ja" "$3")
  se=$(fit_width "$2" 88)
  # 日本語は 72px 基準。ただし**英語より大きくしない**（英語が落ちた行では
  # 「英語 - 4px」を上限にする）。drawl は英語 64px なので日本語 60px になる。
  sj=$(fit_width "$3" 72)
  if [ "$sj" -gt $((se - 4)) ]; then sj=$((se - 4)); fi
  echo "drawtext=fontfile='$FONT':textfile='$fe':fontsize=$se:fontcolor=white:borderw=4:bordercolor=$BG:x=(w-tw)/2:y=(140-th)/2:alpha='$4',drawtext=fontfile='$FONT':textfile='$fj':fontsize=$sj:fontcolor=0xd8d8d8:borderw=4:bordercolor=$BG:x=(w-tw)/2:y=940+(140-th)/2:alpha='$4'"
}

# tagline（C1 / C2。中央・英 104 / 日 64・縁取り 12px。§4）
tagline() { # $1=alpha式
  local fe fj
  fe=$(txt tag_en "$TAG_EN")
  fj=$(txt tag_ja "$TAG_JA")
  echo "drawtext=fontfile='$FONT':textfile='$fe':fontsize=104:fontcolor=white:borderw=12:bordercolor=$BG:x=(w-tw)/2:y=(h-th)/2-70:alpha='$1',drawtext=fontfile='$FONT':textfile='$fj':fontsize=64:fontcolor=0xd8d8d8:borderw=12:bordercolor=$BG:x=(w-tw)/2:y=(h-th)/2+70:alpha='$1'"
}

# 型 A のカット（黒コマ 0.3 → GUI イン 1.0 → 掃引 → コピー FO → GUI FO）。
# コピーは fade より後に描く。順序を誤ると文字までフェードの影響を受ける。
type_a() { # $1=crate $2=out $3=en $4=ja $5.. = --key 群
  local crate=$1 out=$2 en=$3 ja=$4
  shift 4
  local dir="$WORK/${crate}_frames"
  rm -rf "$dir"
  "$SHOTS" anim --crate "$crate" --fps "$FPS" --scale 2.0 "$@" --out "$dir" >/dev/null 2>&1
  local alpha="if(lt(t,1),0,if(lt(t,2),(t-1),if(lt(t,14),1,if(lt(t,15),1-(t-14),0))))"
  "$FFMPEG" -y -v error -framerate "$FPS" -i "$dir/%05d.png" \
    -vf "$pad,fade=t=in:st=0.3:d=1:color=$BG,fade=t=out:st=15:d=1:color=$BG,drawbox=x=0:y=0:w=1920:h=1080:color=$BG@1:t=fill:enable='lt(t,0.3)',$(copy_a "$crate" "$en" "$ja" "$alpha")" \
    -frames:v $((FPS * 16)) "${enc[@]}" "$out"
  rm -rf "$dir" "${dir}.mp4"
}

# ── C1 ────────────────────────────────────────────────────────────────
if want C1; then
  "$FFMPEG" -y -v error -f lavfi -i "color=$BG:s=1920x1080:r=$FPS" -t 4 \
    -vf "$(tagline 'if(lt(t,0.5),0,if(lt(t,1.5),(t-0.5),1))')" "${enc[@]}" "$WORK/c1.mp4"
fi

# ── C2 ────────────────────────────────────────────────────────────────
if want C2; then
  rm -rf "$WORK/c2f"
  "$SHOTS" anim --crate crush --fps "$FPS" --scale 2.0 \
    --key "0:bits=0.4667,rate=0.0,mix=1.0" --key "6:bits=0.4667,rate=1.0,mix=1.0" \
    --out "$WORK/c2f" >/dev/null 2>&1
  # 合焦: gblur の sigma は sendcmd で変えられないため、sigma 固定の gblur を
  # 1 フレーム 1 段で並べ timeline の enable で切り替える（実測 2026-08-28）。
  blur=""
  steps=$(awk -v f="$FPS" 'BEGIN{printf "%d", 0.6*f + 0.5}')
  for i in $(seq 0 $((steps - 1))); do
    sg=$(awk -v i="$i" -v n="$steps" 'BEGIN{u=i/n; printf "%.3f", 24*(1-u)*(1-u)}')
    a=$(awk -v i="$i" -v n="$steps" 'BEGIN{printf "%.4f", 0.6*i/n}')
    b=$(awk -v i="$i" -v n="$steps" 'BEGIN{printf "%.4f", 0.6*(i+1)/n}')
    blur="$blur,gblur=sigma=$sg:enable='between(t,$a,$b)'"
  done
  "$FFMPEG" -y -v error -framerate "$FPS" -i "$WORK/c2f/%05d.png" \
    -vf "$pad$blur,$(tagline 1)" -frames:v $((FPS * 6)) "${enc[@]}" "$WORK/c2.mp4"
  rm -rf "$WORK/c2f" "$WORK/c2f.mp4"
fi

# ── C3 ────────────────────────────────────────────────────────────────
if want C3; then
  fo="if(lt(t,1.4),1,max(0,1-(t-1.4)/0.6))"
  "$FFMPEG" -y -v error -f lavfi -i "color=$BG:s=1920x1080:r=$FPS" -t 2 \
    -vf "drawtext=fontfile='$FONT':text='fandango':fontsize=380:fontcolor=white:x=(w-tw)/2:y=(h-th)/2-90:alpha='$fo',drawtext=fontfile='$FONT':text='by switchworks':fontsize=110:fontcolor=0x9a9a9a:x=(w-tw)/2:y=(h/2)+150:alpha='$fo'" \
    "${enc[@]}" "$WORK/c3.mp4"
fi

# ── C5 / C6 / C7（深掘り 3 本。--key の時刻はカット内相対秒） ──────────
if want C5; then
  type_a nice-bulk "$WORK/c5.mp4" \
    "The further you turn it, the fatter it gets." "上げるほど、太くなる。" \
    --key "0:bass_amount=0.0,hi_amount=0.0" --key "7:bass_amount=0.0,hi_amount=0.0" \
    --key "14:bass_amount=0.6,hi_amount=0.6" --key "16:bass_amount=0.6,hi_amount=0.6"
fi

if want C6; then
  type_a nimbus "$WORK/c6.mp4" \
    "It doesn't widen. It settles things in." "広げるのではなく、馴染ませる。" \
    --key "0:distance=0.0,mix=1.0" --key "2:distance=0.0,mix=1.0" \
    --key "14:distance=1.0,mix=1.0" --key "16:distance=1.0,mix=1.0"
fi

if want C7; then
  type_a raindrop "$WORK/c7.mp4" \
    "That short tail. You already know it." "その短い響きを、知っている。" \
    --key "0:time=0.3421,feedback=0.5556,width=0.0,mix=0.0" \
    --key "2:time=0.3421,feedback=0.5556,width=0.0,mix=0.0" \
    --key "14:time=0.3421,feedback=0.5556,width=0.0,mix=1.0" \
    --key "16:time=0.3421,feedback=0.5556,width=0.0,mix=1.0"
fi

# ── C8（モンタージュ 7 本 × 4 秒。黒コマ無し・直結、crate 名テロップのみ） ──
if want C8; then
  # crate 名テロップは出さない。GUI ヘッダに `fandango : <crate>` が既に出ており、
  # 1 本 4 秒あるなら line（copy.toml）を読ませるほうが情報量が高い（2026-08-31 の改訂）。
  montage() { # $1=crate $2=index $3=en $4=ja $5.. = --key 群
    local crate=$1 idx=$2 en=$3 ja=$4
    shift 4
    local dir="$WORK/c8_${idx}f"
    rm -rf "$dir"
    "$SHOTS" anim --crate "$crate" --fps "$FPS" --scale 2.0 "$@" --out "$dir" >/dev/null 2>&1
    # 切替が 0.2 秒なので、文字が先に立つと切替そのものが見えなくなる。
    # 0.2 秒待ってから 0.3 秒でフェードインする。
    local alpha="if(lt(t,0.2),0,if(lt(t,0.5),(t-0.2)/0.3,1))"
    "$FFMPEG" -y -v error -framerate "$FPS" -i "$dir/%05d.png" \
      -vf "$pad,$(copy_a "c8_$crate" "$en" "$ja" "$alpha")" \
      -frames:v $((FPS * 4)) "${enc[@]}" "$WORK/c8_$idx.mp4"
    rm -rf "$dir" "${dir}.mp4"
  }
  montage spread 1 "It widens. The phase does not move." "広げても、位相は動かない。" \
    --key "0:expander=1.0" --key "4:expander=1.0"
  montage drawl 2 "It never adds gain. The output level follows the material and the vowel." "ゲインを足さない設計。出力レベルは素材と母音に従う。" \
    --key "0:vowel=1.0" --key "0.2:vowel=1.0" --key "3.4:vowel=0.0" --key "4:vowel=0.0"
  montage velvet 3 "Width, made out of mono." "モノから、幅を作る。" \
    --key "0:width=0.35" --key "4:width=0.35"
  # gate のパターンは `#[persist]` でパラメータではないため `--text` で渡す（§7 (1) 残実装 3）。
  montage gate 4 "It cuts exactly where you wrote it." "書いたとおりに、切れる。" \
    --text "pattern=|||-|||-|||||||-" \
    --key "0:duty=0.6,edge=0.06" --key "4:duty=0.6,edge=0.06"
  montage rotate 5 "Same tone. Cut the peaks first." "音色は変えず、山は先に削る。" \
    --key "0:freq=1.0" --key "0.2:freq=1.0" --key "3.4:freq=0.0" --key "4:freq=0.0"
  montage tilt 6 "Bright or dark, one knob." "明暗を、1本で。" \
    --key "0:gain=0.25" --key "0.2:gain=0.25" --key "3.4:gain=0.75" --key "4:gain=0.75"
  montage trip-delay 7 "Left drifts off right, and the beat multiplies." "左右がずれて、拍が増える。" \
    --key "0:level=1.0" --key "4:level=1.0"
  printf "file '%s'\n" c8_1.mp4 c8_2.mp4 c8_3.mp4 c8_4.mp4 c8_5.mp4 c8_6.mp4 c8_7.mp4 >"$WORK/c8list.txt"
  "$FFMPEG" -y -v error -f concat -safe 0 -i "$WORK/c8list.txt" -c copy "$WORK/c8.mp4"
fi

# ── C9（チェーン 2×2、20 秒。finalist の line は相対 12.0 から） ────────
if want C9; then
  rm -rf "$WORK/c9f"
  "$SHOTS" chain --dur 20 --fps "$FPS" --out "$WORK/c9f" \
    --stage "0:nice-bulk:bass_amount=0.2,hi_amount=0.2" \
    --stage "4:gate:duty=0.6,edge=0.35,depth=0.7,pattern=|||-|-||-||-||-|" \
    --stage "8:trip-delay:level=0.77" \
    --stage "12:finalist:input=0.75" >/dev/null 2>&1
  alpha="if(lt(t,12),0,if(lt(t,12.8),(t-12)/0.8,if(lt(t,19),1,1-(t-19))))"
  "$FFMPEG" -y -v error -framerate "$FPS" -i "$WORK/c9f/%05d.png" \
    -vf "$(copy_a finalist "The floor is set at the industry standard." "最低ラインを、業界標準に置いた。" "$alpha")" \
    -frames:v $((FPS * 20)) "${enc[@]}" "$WORK/c9.mp4"
  rm -rf "$WORK/c9f" "$WORK/c9f.mp4"
fi

# ── C4（12 本タイル 6 秒。タイルは 0.4 秒でフェードイン） ───────────────
if want C4; then
  rm -rf "$WORK/c4f"
  # 1 本 0.500 秒は 120 BPM の 1 拍。刻みを崩すと曲の拍と合わなくなるので変えない。
  # 尺は 8.000 秒【再改訂 2026-08-31】。7.000 秒だと C10 が 115.000（奇数秒）から始まり、
  # 120 BPM の小節頭（偶数秒）に乗らず C10 の段階表示が曲と噛まなかった。
  # 8.000 秒にすると C10 は 116.000 から始まり、段の追加 116 / 118 / 120 がすべて小節頭になる。
  # 12 本目の完了は 113.700、そこから 116.000 まで 2.300 秒保持する。
  "$SHOTS" names --dur 8 --fps "$FPS" --start 0 --step 0.5 --wipe 0.2 --out "$WORK/c4f" >/dev/null 2>&1
  "$FFMPEG" -y -v error -framerate "$FPS" -i "$WORK/c4f/%05d.png" \
    -vf "fade=t=in:st=0:d=0.4:color=$BG" -frames:v $((FPS * 8)) "${enc[@]}" "$WORK/c4.mp4"
  rm -rf "$WORK/c4f" "$WORK/c4f.mp4"
fi

# ── C10（エンド 12 秒。116.000–128.000） ──────────────────────────────
# 文言は copy.toml が正本だが、価格と URL は未確定（copy/get.en.md が "Coming soon"）。
# 確定したらここを差し替える。
if want C10; then
  # 冒頭に「黒へ落とす」1 秒を置かない【改訂 2026-08-31】。
  # C4 でタイルが揃うのが 113.700 で、そこから 1.3 秒（C4 末尾の静止 0.3 + この 1.0）
  # 何も無い時間ができ、間延びして見えた。ロゴを 114.000 から出し、
  # タイルを次カットの 1 フレーム前まで見せる。境界はグリッド上に残る。
  # 3 面を差し替えるのをやめ、1 画面へ縦に積んで 3 段階で足していく【改訂 2026-08-31】。
  # 差し替え式は各面が 3〜5 秒で消えるため読み落としが起きる。積み上げなら
  # 4 秒で全情報が出揃い、そこから 7 秒まるごと読める。
  # 段の追加は 2 秒ごと。120 BPM では 2 秒 = 1 小節なので曲と噛む。
  #
  # サイズは優先順位で決める【2026-08-31・ユーザー裁定】:
  #   fandango 240 > tagline 英 104 / 日 64 > 入手先・価格 76（同寸） > 環境 52 / GPL 48
  # 当初は入手先 144 > tagline 英 72 と**逆転していた**（プラットフォーム名が製品の主張より
  # 大きい）。入手先と価格を同寸にしたうえで、色で差を付ける（入手先 #9a9a9a、価格 #d8d8d8）。
  # tagline を初出（C1/C2 の 104/64）と同寸に戻したのは**可読性**の判断
  # （31 文字あり、小さくすると読みにくい。ユーザー選定）。
  # y は入手先行の削除に合わせて詰め直した【2026-08-31・ユーザー裁定 A 案】。
  # 削除で 450→695 に 165px の穴が空き、過剰な余白になった。上群（ロゴ + tagline）を
  # +40 下げ、下群（価格以下）を -55〜-60 上げて群間を 70px にする。
  # **群間を群内（GPL→Windows の 42px）と同じまで詰めない**: この隙間だけが
  # 「識別（ロゴ + tagline）」と「事実（価格・本数・環境）」の切れ目を担っており、
  # 同寸にすると 5 行が 1 つのリストに見え、tagline が仕様の 1 行に落ちる。
  # 上余白 100 / 下余白 108 でほぼ均衡。
  fi1="if(lt(t,0.3),t/0.3,1)"          # 1 段目: 0.0 から
  fi2="if(lt(t,2),0,if(lt(t,2.3),(t-2)/0.3,1))"  # 2 段目: 2.0 から
  fi3="if(lt(t,4),0,if(lt(t,4.3),(t-4)/0.3,1))"  # 3 段目: 4.0 から
  t1="drawtext=fontfile='$FONT':text='fandango':fontsize=240:fontcolor=white:x=(w-tw)/2:y=100:alpha='$fi1'"
  # tagline は英日を同時に出す（決定 5 / §5.5 C10「tagline 英日 再掲」）。
  t2="drawtext=fontfile='$FONT':text='$TAG_EN':fontsize=104:fontcolor=white:x=(w-tw)/2:y=370:alpha='$fi1'"
  t2b="drawtext=fontfile='$FONT':textfile='$(txt end_tag_ja "$TAG_JA")':fontsize=64:fontcolor=0xd8d8d8:x=(w-tw)/2:y=490:alpha='$fi1'"
  # 価格は独立した行で出す。価格の記載が無いと既定で有料と読まれる
  # （Gumroad は有料販売の場として認知されている）。GPL-3 は再配布の自由であって価格ではない。
  # name-your-price（$0+、plans/release_marketing_v01.md）なので 2 文で言い切る。
  # 入手先（プラットフォーム名）は動画に出さない【裁定 2026-08-31】。
  # URL を入れないのと同じ理由で、プラットフォーム名も古びる（畳まれる・移る・別導線になる）。
  # 動画に恒久的に焼くのは製品の事実だけ（無料・12 本・GPL-3・Windows・VST3）。
  # どこで配るかは概要欄とページの仕事で、ソースの所在は GPL-3. から辿れる。
  t3b="drawtext=fontfile='$FONT':text='Free. Name your price.':fontsize=76:fontcolor=0xd8d8d8:x=(w-tw)/2:y=640:alpha='$fi2'"
  t4="drawtext=fontfile='$FONT':text='12 plugins. GPL-3.':fontsize=48:fontcolor=0x9a9a9a:x=(w-tw)/2:y=745:alpha='$fi2'"
  # foobar2000 は動画に出さない【裁定 2026-08-31】。
  # fb2k 版は 4 本のみ（crush / nice-bulk / spread / tilt。`crates/fb2k-ffi/src/lib.rs` を実測）で、
  # 「12 plugins.」の直下に「VST3 / foobar2000」を並べると**12 本が両形式で出ると読まれる**。
  # かつ fb2k 版はおまけ（appendix）扱いであり、動画は主張が最も強い面なので載せると実態より重い。
  # VST3 が 12 本というのは事実なので、それだけを言う。fb2k はページ側に appendix として書く。
  t5="drawtext=fontfile='$FONT':text='Windows 10 / 11 (64-bit)':fontsize=52:fontcolor=0x9a9a9a:x=(w-tw)/2:y=845:alpha='$fi3'"
  t6="drawtext=fontfile='$FONT':text='VST3':fontsize=52:fontcolor=0x9a9a9a:x=(w-tw)/2:y=910:alpha='$fi3'"
  "$FFMPEG" -y -v error -f lavfi -i "color=$BG:s=1920x1080:r=$FPS" -t 12 \
    -vf "$t1,$t2,$t2b,$t3b,$t4,$t5,$t6,fade=t=out:st=10:d=2:color=black" "${enc[@]}" "$WORK/c10.mp4"
fi

# ── 結合と音の多重化 ──────────────────────────────────────────────────
# 欠けたカットがあれば結合へ進まない。concat は入力が無くてもエラーを吐いたまま
# 進んでしまい、古い video.mp4 から不完全な成果物が出る（実測 2026-08-31）。
cuts=(c1.mp4 c2.mp4 c3.mp4 c5.mp4 c6.mp4 c7.mp4 c8.mp4 c9.mp4 c4.mp4 c10.mp4)
missing=()
for f in "${cuts[@]}"; do
  [ -f "$WORK/$f" ] || missing+=("$f")
done
if [ ${#missing[@]} -gt 0 ]; then
  echo "カットが揃っていないので結合しない: ${missing[*]}" >&2
  echo "（PROMO_ONLY で一部だけ作った直後は正常。全部揃えてから再実行すること）" >&2
  exit 1
fi

printf "file '%s'\n" "${cuts[@]}" >"$WORK/list.txt"
"$FFMPEG" -y -v error -f concat -safe 0 -i "$WORK/list.txt" -c copy "$WORK/video.mp4"

# 楽曲は 126.000 秒。動画の 128.000 秒との差は無音でパディングする（§5.6）。
"$FFMPEG" -y -v error -i "$WORK/video.mp4" -i "$WAV" \
  -af "apad=whole_dur=128" -map 0:v -map 1:a -c:v copy -c:a aac -b:a 320k -t 128 "$OUT"

echo -n "尺: "
"$FFPROBE" -v error -show_entries format=duration -of default=nw=1:nk=1 "$OUT"
echo "$OUT"
