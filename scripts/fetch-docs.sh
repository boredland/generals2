#!/usr/bin/env bash
# Idempotent: existing non-empty outputs are skipped. Failures go to archive/docs/FAILED.txt.
# Usage: scripts/fetch-docs.sh [wikipedia|fandom|press|reddit|artstation|moddb ...]   (default: all)
set -uo pipefail
cd "$(dirname "$0")/.."

OUT=archive/docs
FAILED="$OUT/FAILED.txt"
UA="Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0"
FANDOM=https://cnc.fandom.com/api.php
mkdir -p "$OUT"/{fandom/images,press,reddit,moddb,artstation}
touch "$FAILED"

fail() { printf '%s\t%s\t%s\n' "$1" "$2" "$3" >> "$FAILED"; }

# Fetches $1 into $2; on non-2xx retries via the Wayback Machine (any capture, newest first). Returns non-zero if all fail.
fetch() {
  local url=$1 dest=$2 code wb
  [ -s "$dest" ] && return 0
  code=$(curl -sL --compressed -A "$UA" -o "$dest" -w '%{http_code}' --max-time 60 "$url")
  [[ $code == 2* ]] && [ -s "$dest" ] && return 0
  wb=$(curl -sL --compressed -A "$UA" -o "$dest" -w '%{http_code}' --retry 3 --retry-all-errors --max-time 240 \
    "https://web.archive.org/web/2id_/$url")
  if [[ $wb == 2* ]] && [ -s "$dest" ]; then
    printf '%s\n' "$url" > "$dest.wayback"
    return 0
  fi
  rm -f "$dest"
  fail "$url" "$code" "wayback fallback: $wb"
  return 1
}

slug() { printf '%s' "$1" | sed -E 's#^https?://##; s#[^A-Za-z0-9._-]+#_#g; s#_+$##' | cut -c1-150; }

# HTML → readable Markdown next to it (keeps the raw HTML as the source of truth).
to_md() {
  local html=$1 md=${1%.html}.md
  [ -s "$md" ] && return 0
  pandoc -f html -t gfm-raw_html --wrap=none "$html" -o "$md" 2>/dev/null || rm -f "$md"
}

run_wikipedia() {
  for title in "Command_%26_Conquer_(cancelled_video_game)"; do
    fetch "https://en.wikipedia.org/w/index.php?title=$title&action=raw" "$OUT/wikipedia-generals2.wiki"
  done
  # Redirect stubs, kept so the old article titles are traceable.
  fetch "https://en.wikipedia.org/w/index.php?title=Command_%26_Conquer:_Generals_2&action=raw" "$OUT/wikipedia-redirect-generals2.wiki"
  fetch "https://en.wikipedia.org/w/index.php?title=Command_%26_Conquer_(2013_video_game)&action=raw" "$OUT/wikipedia-redirect-2013.wiki"
}

fandom_api() { curl -s --compressed -A "$UA" --max-time 60 "$FANDOM?format=json&$1"; }

fandom_page() {
  local title=$1 dest
  dest="$OUT/fandom/$(printf '%s' "$title" | tr '/ :' '__-').wiki"
  [ -s "$dest" ] && return 0
  local enc
  enc=$(jq -rn --arg t "$title" '$t|@uri')
  fandom_api "action=parse&page=$enc&prop=wikitext&redirects=1" | jq -r '.parse.wikitext["*"] // empty' > "$dest"
  [ -s "$dest" ] || { rm -f "$dest"; fail "fandom:$title" "-" "parse returned no wikitext"; }
}

fandom_file() {
  local title=$1 enc url name
  name=${title#File:}
  [ -s "$OUT/fandom/images/$name" ] && return 0
  enc=$(jq -rn --arg t "$title" '$t|@uri')
  url=$(fandom_api "action=query&titles=$enc&prop=imageinfo&iiprop=url" | jq -r '.query.pages[].imageinfo[0].url // empty')
  [ -n "$url" ] || { fail "fandom:$title" "-" "no imageinfo url"; return; }
  curl -sL -A "$UA" --max-time 120 -o "$OUT/fandom/images/$name" "$url" || fail "$url" "-" "image download failed"
}

# Walks a category recursively; pages → wikitext, files → originals. $SEEN guards against category cycles.
declare -A SEEN
fandom_category() {
  local cat=$1
  [ -n "${SEEN[$cat]:-}" ] && return
  SEEN[$cat]=1
  local enc
  enc=$(jq -rn --arg t "$cat" '$t|@uri')
  fandom_api "action=query&list=categorymembers&cmtitle=$enc&cmlimit=500" \
    | jq -r '.query.categorymembers[] | "\(.ns)\t\(.title)"' \
    | while IFS=$'\t' read -r ns title; do
        case $ns in
          14) fandom_category "$title" ;;
          6) fandom_file "$title" ;;
          *) fandom_page "$title" ;;
        esac
      done
}

run_fandom() {
  for p in "Command & Conquer (2013)" "Generals 2 animatics" "Unreleased content (Generals 2)" \
           "European Union" "Asia-Pacific Alliance" "Global Liberation Army" "Supply center (Generals 2)" \
           "Generals multiplayer sneak peek" "Generals Evolution" "Command & Conquer: Generals 2 (mod)"; do
    fandom_page "$p"
  done
  for c in "Category:Generals 2 units" "Category:Generals 2 GLA arsenal" \
           "Category:Generals 2 concept art" "Category:Generals 2 images"; do
    fandom_category "$c"
  done
}

press=(
  https://www.ea.com/en-ca/news/ea-unveils-command-conquer-generals-2
  https://s204.q4cdn.com/701424631/files/doc_news/2012/08/1/700571.pdf
  "https://news.ea.com/press-releases/press-releases-details/2013/EA-and-Victory-Games-Put-Players-in-Control-with-a-New-Generation-of-Command--Conquer/default.aspx"
  https://www.polygon.com/2013/10/29/5043244/
  https://www.gamespot.com/articles/ea-cancels-free-to-play-command-conquer-game-closes-studio/1100-6415840/
  https://kotaku.com/ea-cancels-command-conquer-1454179860
  https://mcvuk.com/development-news/ea-disbands-victory-studios/
  https://sea.ign.com/pc/65528/
  https://criticalmovespodcast.com/generals-2-the-game-that-never-was/
  https://cncnz.com/features/other-specials/cancelled-command-conquer-games/
  https://forums.cncnz.com/topic/20621-former-ea-dev-shares-thoughts-on-the-cancelled-cc-2013/
  https://forums.cncnz.com/topic/21053-early-black-friday-giveaway-the-canceled-generals-2-f2p-game/
  https://www.moddb.com/news/interview-with-ea-cire-regarding-cc-generals-2
  https://www.gamereplays.org/generals2/
  "https://www.gamereplays.org/cnczerohour/portals.php?name=generals-gentlemen-firstlook-Generals2-Alpha&show=page"
  https://forums.cncnet.org/topic/1523-generals-2/
  https://www.cnclabs.com/generals2/
  https://www.cncsaga.com/en/wiki/jeu/cnc-2013
  https://www.cncsaga.com/en/wiki/jeu/cnc-2013/faction/europe
  https://www.cncsaga.com/en/wiki/jeu/cnc-2013/faction/gla
  https://www.cncsaga.com/en/wiki/jeu/cnc-2013/faction/apa
  https://www.cncsaga.com/en/wiki/jeu/cnc-2013/galerie/Concept-art
  https://www.cncsaga.com/en/wiki/jeu/cnc-2013/galerie/Interface
  https://www.cncsaga.com/en/wiki/jeu/cnc-2013/galerie/Screenshot
)

run_press() {
  local url dest
  for url in "${press[@]}"; do
    if [[ $url == *.pdf ]]; then
      fetch "$url" "$OUT/press/$(slug "$url")"
    else
      dest="$OUT/press/$(slug "$url").html"
      fetch "$url" "$dest" && to_md "$dest"
    fi
  done
}

# Reddit 403s unauthenticated JSON; fall back to the newest Wayback capture of the thread's HTML page.
run_reddit() {
  local id cap stamp orig dest
  for id in fu5hmw 29ozx7 1dk8k6 u33z8j; do
    dest="$OUT/reddit/$id.json"
    [ -s "$dest" ] || [ -s "$OUT/reddit/$id.html" ] && continue
    code=$(curl -sL -A "$UA" -o "$dest" -w '%{http_code}' --max-time 60 "https://www.reddit.com/r/commandandconquer/comments/$id.json")
    [[ $code == 2* ]] && continue
    rm -f "$dest"
    cap=$(curl -s --retry 3 --retry-all-errors --max-time 120 \
      "https://web.archive.org/cdx/search/cdx?url=reddit.com/r/commandandconquer/comments/$id/*&fl=timestamp,original&filter=statuscode:200&filter=mimetype:text/html&output=json" \
      | jq -r 'if type == "array" and length > 1 then .[-1] | join(" ") else empty end' 2>/dev/null)
    if [ -z "$cap" ]; then fail "reddit:$id" "$code" "no Wayback HTML capture (or CDX unavailable)"; continue; fi
    read -r stamp orig <<< "$cap"
    dest="$OUT/reddit/$id.html"
    if curl -sfL --compressed -A "$UA" --retry 3 --max-time 240 -o "$dest" "https://web.archive.org/web/${stamp}id_/$orig"; then
      printf '%s\n' "$orig" > "$dest.wayback"
      to_md "$dest"
    else
      rm -f "$dest"; fail "reddit:$id" "$code" "Wayback fetch of $stamp failed"
    fi
  done
}

# https://www.artstation.com/epolak was checked on 2026-10-23: nothing on the profile is attributed to C&C, so it is not fetched.
artstation=(
  https://wasabirobot.artstation.com/projects/wOxQ9
  https://wasabirobot.artstation.com/projects/Rq2yO
  https://www.artstation.com/artwork/B1ePX9
  https://www.artstation.com/artwork/n8v1K
)

run_artstation() {
  local url
  for url in "${artstation[@]}"; do
    uvx gallery-dl --write-metadata -d "$OUT/artstation" "$url" || fail "$url" "-" "gallery-dl failed"
  done
}

moddb=(
  https://www.moddb.com/mods/command-and-conquer-generals-evolution
  https://www.moddb.com/mods/cc-generals-revolution
  https://www.moddb.com/company/command-conquer-generals-2-mod-team/downloads
  https://www.moddb.com/mods/cncgenerals2-remastered
  https://www.moddb.com/mods/generals-2
  https://www.moddb.com/groups/c-c-paradise/videos/generals-2-ost-european-union-04
)

run_moddb() {
  local url dest
  for url in "${moddb[@]}"; do
    dest="$OUT/moddb/$(slug "$url").html"
    fetch "$url" "$dest" && to_md "$dest"
  done
}

sections=("$@")
[ ${#sections[@]} -eq 0 ] && sections=(wikipedia fandom press reddit artstation moddb)
for s in "${sections[@]}"; do "run_$s"; done
sort -u -o "$FAILED" "$FAILED"
