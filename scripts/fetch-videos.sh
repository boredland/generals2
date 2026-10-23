#!/usr/bin/env bash
# Idempotent: yt-dlp skips IDs listed in archive/videos/.downloaded.txt, `ia download` skips existing files.
# Usage: scripts/fetch-videos.sh [official|alpha|retro|playlist|sweep|ia|moddb ...]   (default: all)
set -uo pipefail
cd "$(dirname "$0")/.."

YTDLP=(uvx yt-dlp)
IA=(uvx --from internetarchive ia)
OUT=archive/videos
mkdir -p "$OUT"
FAILED="$OUT/FAILED.txt"
touch "$FAILED"

COMMON=(-f "bv*[height<=1080]+ba/b" --merge-output-format mkv
  --write-info-json --write-description --write-thumbnail --write-subs --write-auto-subs --sub-langs "en.*,de,ru"
  --embed-metadata --download-archive "$OUT/.downloaded.txt"
  --sleep-requests 1 --sleep-interval 2 --max-sleep-interval 6 --retries 10 --ignore-errors --no-overwrites
  ${YTDLP_EXTRA:-})

# / is shared with other services on this host; stop before downloads can fill it. Override with MIN_FREE_GB.
MIN_FREE_GB=${MIN_FREE_GB:-30}
disk_ok() {
  local free
  free=$(df -BG --output=avail "$OUT" | tail -n 1 | tr -dc 0-9)
  if [ "$free" -lt "$MIN_FREE_GB" ]; then
    printf 'disk\t-\tstopped: %s GB free < %s GB\n' "$free" "$MIN_FREE_GB" >> "$FAILED"
    echo "fetch-videos: only ${free} GB free (< ${MIN_FREE_GB} GB), stopping" >&2
    return 1
  fi
}

# Subtitle 429s must not mark the video as failed; only "ERROR: [youtube] <id>:" lines name a failed video.
ytdl() {
  local section=$1; shift
  local log
  disk_ok || exit 1
  log=$(mktemp)
  "${YTDLP[@]}" "${COMMON[@]}" -o "$OUT/$section/%(upload_date)s - %(channel)s - %(title).120s [%(id)s].%(ext)s" "$@" 2>&1 | tee "$log"
  grep -E '^ERROR: \[[a-z:]+\] [A-Za-z0-9_-]{11}:' "$log" | sed "s/^/$section\t/" >> "$FAILED"
  rm -f "$log"
}

official=(w6bg6yhyTK4 QYkkit4Cfq4 a0T7GELJ2oU gNm1cZj4vio igw9Pw33_2s kjZEInQZA0c 0UFXD4dDRvI 80_x6bmzluc
  M77aoRf4MmI Q1xJIB-wN1g lH73IJkA7ag yEAOW-xnwfA XbK1oAmbj04 67KYmGYnC28 SFCjH1YtVRc hpdYAUlmxn8 sBuAtGkqX_A
  HQXvf3ZnL-U y-LJZVIeWCQ aHkV0zJncjE -MFAToIYoq8 UBK8OrTQ9Ug JsJdZKisHZc 5rTK_DZG-oE l1HJD6iJUDo)
alpha=(dtl2Q_tUDDQ 1D308eoBp5Q mcPM2vfY-iQ p6KQBkZdUmk xKZ709dP45o 2qBK8sXk-xw rdpesZNWJNw 6XKby6Qz5oI 3tlbDYSUboc
  1-NiSusskDM eJ0W0LXiX1c V8eIhlzuXm4 aqPIdxWCCSw r-x34dGj8BE rEwSDh4LZaY 6Q7BCgNHgGg IwMaqDQCatY)
retro=(TAbtp3btcsQ Esaj69pnODI zNjbm9Y1k80 VZC9PzT92-U _fCAaPWLlpk)
# Cited by archive/docs/fandom/*.wiki ({{#vlink}}) or linked from archive/docs/reddit/ threads, not covered above;
# 2011-2013 official/dev/alpha footage only.
wiki=(g1EG5tc5PM0 N16AFzdMHDY -9zVjFcpjeA S-6y6RwlqAg A1IJml_vdas BWoFaLb3flY 98bXedVB6tM k1oqJ5-Re7E A9g_pat78eA
  g9MHcjaf0NM rRZMAgRe7uA XVWEx3h5fCs 2jOBKte96mQ)
# Original *Generals* (2002) multiplayer sneak peek, cited by fandom/Generals_multiplayer_sneak_peek.wiki.
# Not Generals 2: kept apart as SAGE-era design reference.
generals1_beta=(999a4Tk-z8E BwjNn-xNdUk IwLHSIz7oCM M1rxJvIp-HQ MHGgk746eYo O_MduTl5lb8 ed6pgV1-q2c Xl9Mu8jIDCI)
ia_items=(g4tv.com-video56599-flvhd g4tv.com-video56599 tweakers004492 tweakers006092 tweakers008132 tweakers008160
  roosterteeth-6512 roosterteeth-6800 youtube-a0T7GELJ2oU youtube-w6bg6yhyTK4 youtube-lH73IJkA7ag youtube-XbK1oAmbj04
  youtube-Q1xJIB-wN1g youtube-zNjbm9Y1k80)
moddb=(https://www.moddb.com/groups/c-c-paradise/videos/generals-2-ost-european-union-04)

ids_to_urls() { for id in "$@"; do printf 'https://www.youtube.com/watch?v=%s\n' "$id"; done; }

run_official() { ytdl official $(ids_to_urls "${official[@]}"); }
run_alpha()    { ytdl alpha $(ids_to_urls "${alpha[@]}"); }
run_retro()    { ytdl retro $(ids_to_urls "${retro[@]}"); }
run_wiki()     { ytdl wiki $(ids_to_urls "${wiki[@]}"); }
run_generals1_beta() { ytdl generals1-beta $(ids_to_urls "${generals1_beta[@]}"); }
run_playlist() { ytdl playlist "https://www.youtube.com/playlist?list=PLFEB85F247260EAD5"; }
# One yt-dlp call per query so the disk guard runs between queries; per-file cap bounds the worst case.
run_sweep() {
  local q
  for q in 'ytsearch80:command and conquer generals 2' \
           'ytsearch80:"command & conquer 2013" alpha' \
           'ytsearch40:"generals 2" alpha gameplay' \
           'ytsearch40:"command and conquer" "victory games"' \
           'ytsearch40:"generals 2" cancelled'; do
    ytdl sweep --match-filter "duration < 3600" --max-filesize 1500M \
      --reject-title "(?i)zero hour|rise of the reds|contra|red alert|tiberium|remastered|shockwave" "$q"
  done
}
run_ia() {
  mkdir -p "$OUT/ia"
  for item in "${ia_items[@]}"; do
    disk_ok || exit 1
    "${IA[@]}" download "$item" --glob '*.mp4|*.mkv|*.flv|*.webm|*.json|*.description' --destdir "$OUT/ia" \
      || printf 'ia\t%s\tia download failed\n' "$item" >> "$FAILED"
  done
}
run_moddb() { ytdl moddb "${moddb[@]}"; }

sections=("$@")
[ ${#sections[@]} -eq 0 ] && sections=(official alpha retro wiki generals1_beta playlist sweep ia moddb)
for s in "${sections[@]}"; do "run_$s"; done
sort -u -o "$FAILED" "$FAILED"
