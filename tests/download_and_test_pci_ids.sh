#!/bin/bash
set -euo pipefail

# Download pci.ids and keep NVIDIA Hopper+ (23xx,29xx-2fxx,3xxx+) and Mellanox (15b3)
URL="https://pci-ids.ucw.cz/v2.2/pci.ids"
OUT="${1:-tests/data/pci.ids}"
TMP=$(mktemp)

echo "Fetching $URL -> $OUT"
mkdir -p "$(dirname "$OUT")"
curl -sSL "$URL" -o "$TMP"
[[ -s "$TMP" ]] || { echo "empty download" >&2; rm -f "$TMP"; exit 1; }

awk '
BEGIN{nvidia=0;mellanox=0;printed_nv=0;parent_printed=0}
/^#/||/^$/ {print;next}
/^10de  /{nv_line=$0;nvidia=1;mellanox=0;parent_printed=0;printed_nv=0;next}
/^15b3  /{print;nvidia=0;mellanox=1;next}
/^[0-9a-f]{4}  /{nvidia=0;mellanox=0;next}
/^\t[^\t]/ {
  if(nvidia){ 
    id=substr($1,1,4)
    if(id ~ /^(23|2[9a-f]|[3-9a-f])/) {
      if(!printed_nv){print nv_line; printed_nv=1}
      print; parent_printed=1
    } else {parent_printed=0}
    next
  }
  if(mellanox){ print; next }
  next
}
/^\t\t/ { if((nvidia && parent_printed) || mellanox) print; next }
' "$TMP" > "$OUT"

[[ -s "$OUT" ]] || { echo "filter produced empty file" >&2; rm -f "$TMP"; exit 1; }

orig=$(wc -l < "$TMP")
flt=$(wc -l < "$OUT")
total_devs=$(grep -c $'^\t[0-9a-f]' "$OUT")
hopper=$(awk $'/^\t23/ {count++} END {print count+0}' "$OUT")
blackwell=$(awk $'/^\t2[9a-f]/ {count++} END {print count+0}' "$OUT")
future=$(awk $'/^\t[3-9a-f]/ {count++} END {print count+0}' "$OUT")
nv_devs=$((hopper + blackwell + future))

echo "Original: $orig lines -> Filtered: $flt lines"
echo "Total devices: $total_devs (NVIDIA: $nv_devs, Mellanox: $((total_devs - nv_devs)))"
echo "NVIDIA breakdown: Hopper($hopper) + Blackwell($blackwell) + Future($future)"

rm -f "$TMP"