#!/usr/bin/env bash
# usage: gitleaks-find.sh PATH...   stages the paths, scans each staged file's diff separately, prints files with findings (no values), unstages
cd "C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b" || exit 1
git add -- "$@" 2>/dev/null
git diff --cached --name-only > .plan-work/staged.txt
while IFS= read -r f; do
  n=$(git diff --cached -- "$f" | MSYS_NO_PATHCONV=1 docker run --rm -i -v "C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/.gitleaks.toml:/gitleaks.toml:ro" ghcr.io/gitleaks/gitleaks:latest stdin --redact --config /gitleaks.toml --exit-code 0 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep -c "leaks found: [1-9]")
  [ "$n" != "0" ] && echo "FINDING $f"
done < .plan-work/staged.txt
git reset -q -- "$@" 2>/dev/null
echo scanned
