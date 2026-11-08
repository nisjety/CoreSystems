#!/usr/bin/env bash
# usage: provenance.sh "<repo-relative path>" ["<path2>" ...]
# Prints: C | M | U | D | MISSING  plus path. Reads the LIVE tree C:/dev/CoresSystem (read-only).
LIVE="C:/dev/CoresSystem"
TSV="$(dirname "$0")/../live-uncommitted.tsv"
for p in "$@"; do
  p="${p%%:*}"   # drop :line suffix if given
  if [ -e "$LIVE/$p" ]; then
    st=$(awk -F'\t' -v p="$p" '{ q=$2; if (q==p) {print $1; exit} n=length(q); if (substr(q,n,1)=="/" && index(p,q)==1) {print $1; exit} }' "$TSV")
    if [ "$st" = "??" ]; then echo "U  $p   (untracked: uncommitted work in live tree)"
    elif [ -n "$st" ]; then echo "M  $p   (committed at afb83928, modified in live tree)"
    elif git -C "$LIVE" --no-optional-locks cat-file -e "afb83928:$p" 2>/dev/null; then echo "C  $p"
    else echo "?  $p   (exists in live tree, not in afb83928, not in status list)"; fi
  else
    st=$(awk -F'\t' -v p="$p" '$2==p {print $1; exit}' "$TSV")
    if [ "$st" = "D" ]; then echo "D  $p   (deleted in live tree)"; else echo "MISSING  $p"; fi
  fi
done
