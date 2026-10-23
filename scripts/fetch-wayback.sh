#!/usr/bin/env bash
# Mirrors commandandconquer.com (2012–2014) from the Wayback Machine into archive/wayback/.
# Enumerates captures via the CDX API (saved as cdx-2012-2014.csv), then fetches each URL's raw
# capture (id_) at its listed timestamp. Idempotent: files already on disk are skipped.
# hartator/wayback-machine-downloader was tried first; it aborts on the first CDX 503
# (OpenURI::HTTPError) and the Archive returns those intermittently, so this does its own retries.
set -uo pipefail
cd "$(dirname "$0")/.."
OUT=archive/wayback
SITE=www.commandandconquer.com
CSV="$OUT/cdx-2012-2014.csv"
FAILED="$OUT/FAILED.txt"
mkdir -p "$OUT/$SITE"
: > "$FAILED"

# Retries on 5xx and on the "Temporarily Offline" HTML the Archive serves with status 200.
wb_get() {
  local url=$1 dest=$2 i
  for i in 1 2 3 4 5; do
    if curl -sfL --compressed --max-time 180 -o "$dest" "$url" && ! grep -qs "Internet Archive: Temporarily Offline" "$dest"; then
      return 0
    fi
    sleep $((i * 10))
  done
  rm -f "$dest"
  return 1
}

if [ ! -s "$CSV" ]; then
  tmp=$(mktemp)
  echo "original,timestamp,mimetype,length" > "$tmp"
  for prefix in "commandandconquer.com/en/*" "commandandconquer.com/de/*" \
                "commandandconquer.com/media/*" "commandandconquer.com/beta-agreement/*"; do
    wb_get "https://web.archive.org/cdx/search/cdx?url=$prefix&from=2012&to=2014&fl=original,timestamp,mimetype,length&collapse=urlkey&filter=statuscode:200" "$tmp.part" \
      || { echo "CDX failed for $prefix" >&2; exit 1; }
    awk 'NF==4 {gsub(/"/, "\"\"", $1); print "\"" $1 "\"," $2 "," $3 "," $4}' "$tmp.part" >> "$tmp"
  done
  mv "$tmp" "$CSV"; rm -f "$tmp.part"
fi

# Maps a captured URL to a local path: strip scheme/port/query, directories become dir/index.html.
local_path() {
  local p=${1#*://}
  p=${p#*/}; p=${p%%\?*}; p=${p%%#*}
  if [ -z "$p" ] || [[ $p == */ ]] || [[ ${p##*/} != *.* ]]; then p="${p%/}/index.html"; p=${p#/}; fi
  printf '%s/%s/%s' "$OUT" "$SITE" "$p"
}

n=0
# Split from the right: only the quoted URL column can contain commas.
tail -n +2 "$CSV" | while IFS= read -r line; do
  len=${line##*,}; line=${line%,*}; mime=${line##*,}; line=${line%,*}; stamp=${line##*,}; orig=${line%,*}
  orig=${orig#\"}; orig=${orig%\"}; orig=${orig//\"\"/\"}
  case $mime in text/html|image/*|text/css|application/pdf) ;; *) continue ;; esac
  dest=$(local_path "$orig")
  [ -s "$dest" ] && continue
  mkdir -p "$(dirname "$dest")"
  wb_get "https://web.archive.org/web/${stamp}id_/$orig" "$dest" \
    || printf '%s\t%s\tfetch failed\n' "$orig" "$stamp" >> "$FAILED"
  n=$((n + 1)); [ $((n % 25)) -eq 0 ] && echo "$n fetched"
  sleep 1
done

priority=(
  "en/news/1380/a-new-future-for-command-conquer" en/news/856 en/news/877 en/news/886 en/news/928
  en/news/1019 en/news/1207 en/news/1280 en/news/1305 en/news/1354 en/news/1365
  en/games/bygameid/cncgenerals2 en/media/screenshot en/media/video en/media/wallpapers
  en/beta-registration beta-agreement/en_legal.html
)
for p in "${priority[@]}"; do
  [ -n "$(find "$OUT/$SITE/$p" -type f 2>/dev/null | head -n 1)" ] && continue
  q=$(mktemp)
  if ! wb_get "https://web.archive.org/cdx/search/cdx?url=commandandconquer.com/$p*&from=2012&to=2014&fl=timestamp,original&filter=statuscode:200&limit=1" "$q" \
     || [ ! -s "$q" ]; then
    printf '%s\t-\tno 200 capture 2012-2014\n' "$p" >> "$FAILED"; rm -f "$q"; continue
  fi
  read -r stamp orig < "$q"; rm -f "$q"
  dest=$(local_path "$orig")
  mkdir -p "$(dirname "$dest")"
  wb_get "https://web.archive.org/web/${stamp}id_/$orig" "$dest" \
    || printf '%s\t%s\tfetch failed\n' "$p" "$stamp" >> "$FAILED"
done
sort -u -o "$FAILED" "$FAILED"
echo "files: $(find "$OUT/$SITE" -type f | wc -l), failed: $(wc -l < "$FAILED")"
