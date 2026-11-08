#!/usr/bin/env bash
# leakscan.sh LISTFILE -> prints the number of gitleaks findings in the staged diff of the listed paths (no values)
cd "C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b" || exit 1
mapfile -t P < "$1"
git diff --cached -- "${P[@]}" | MSYS_NO_PATHCONV=1 timeout 300 docker run --rm -i -v "C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/.gitleaks.toml:/gitleaks.toml:ro" ghcr.io/gitleaks/gitleaks:latest stdin --redact --config /gitleaks.toml --exit-code 0 --no-banner 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep -oE "leaks found: [0-9]+|no leaks found" | head -1
