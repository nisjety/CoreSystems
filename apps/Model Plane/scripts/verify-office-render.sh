#!/usr/bin/env bash
# verify-office-render.sh — WS19-S0 spike: does LibreOffice convert Office files
# under the execution-core bwrap argv, inside the budget that decides D5
# (LibreOffice in the sandbox image, or a sidecar)?
#
# Runs in a throwaway debian:bookworm-slim container as uid 10001, under the
# argv sandbox.rs assemble_argv emits for WorkspaceWrite{[/work]} with network
# Disabled, plus one read-only extra bind of a throwaway skills directory onto
# /skills (LaunchOptions.extra_ro_binds). LibreOffice is installed only inside
# that container; the Model Plane image is not changed here (WS19-S4 does that).
#
# Gates (exit 1 when one fails):
#   - the four good fixtures convert to a PDF (exit 0, %PDF header)
#   - cold start (first conversion, fresh profile) <= 25 s
#   - peak RSS of the sandboxed process tree <= 1 GiB
#   - the sandbox has no network interface other than lo
#   - /skills and the root are read-only inside the sandbox
# The six broken fixtures are converted and reported, not gated: whether
# LibreOffice rejects them is an input to the S2 structural verifier, not a
# spike gate.
#
# Usage:    bash scripts/verify-office-render.sh
# Requires: docker (the container runs --privileged: bwrap needs unprivileged
#           user namespaces, the same constraint as verify-sandbox-isolation.sh).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FIXTURES="$HERE/../rust/services/execution-core/tests/fixtures/office"
CONTAINER="verevon-office-spike-$$"

[ -d "$FIXTURES" ] || { echo "FAIL: fixture directory missing: $FIXTURES"; exit 1; }

cleanup() { docker rm -f "$CONTAINER" >/dev/null 2>&1 || true; }
trap cleanup EXIT

docker run -d --name "$CONTAINER" --privileged debian:bookworm-slim sleep 1800 >/dev/null
docker cp "$FIXTURES/." "$CONTAINER:/fixtures"

docker exec -i "$CONTAINER" bash -s <<'SPIKE'
set -uo pipefail
fail=0
ok() { echo "ok: $*"; }
bad() { echo "FAIL: $*"; fail=1; }
info() { echo "info: $*"; }

echo "VERIFY LibreOffice under the execution-core bwrap argv (WS19-S0)"

used_before=$(df -B1M --output=used / | tail -n 1 | tr -d ' ')
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq >/dev/null 2>&1
apt-get install -y -qq --no-install-recommends bubblewrap time \
  libreoffice-core libreoffice-writer libreoffice-calc libreoffice-impress \
  fonts-liberation fonts-dejavu-core >/dev/null 2>&1 \
  || { bad "apt install of bubblewrap and LibreOffice failed"; exit 1; }
apt-get clean
used_after=$(df -B1M --output=used / | tail -n 1 | tr -d ' ')
info "image delta (df, approximate, MiB): $((used_after - used_before))"

# uid 10001 matches the Model Plane image; /work is the one writable root;
# /skills must pre-exist on the read-only root for the bind to land.
useradd -u 10001 -m -s /bin/sh verevon 2>/dev/null || true
mkdir -p /work/home /work/out /skills /opt/skills-spike
printf '# spike skill\n' > /opt/skills-spike/SKILL.md
chown -R 10001:10001 /work

# The argv sandbox.rs assemble_argv emits for WorkspaceWrite{[/work]}, network
# Disabled, with one extra read-only bind. Same options, same order.
base_argv() {
  printf '%s\n' \
    --die-with-parent --unshare-pid --unshare-uts --unshare-ipc --new-session \
    --dev /dev --tmpfs /tmp --ro-bind / / \
    --clearenv --setenv PATH /usr/local/bin:/usr/bin:/bin \
    --setenv HOME /work/home --setenv TMPDIR /tmp --setenv LANG C.UTF-8 \
    --bind /work /work \
    --ro-bind /opt/skills-spike /skills \
    --unshare-net --chdir /work --
}
mapfile -t ARGV < <(base_argv)
as_verevon() { setpriv --reuid=10001 --regid=10001 --clear-groups "$@"; }

# convert <fixture> <profile-dir>: one sandboxed soffice run, measured.
# Sets RC, ELAPSED (s) and RSS_KB (peak of the sandboxed process tree).
convert() {
  local name="$1" profile="$2" stem
  stem="${name%.*}"
  rm -f "/work/out/$stem.pdf" /work/time.txt
  as_verevon /usr/bin/time -f '%e %M' -o /work/time.txt \
    bwrap "${ARGV[@]}" soffice --headless --norestore \
      "-env:UserInstallation=file://$profile" \
      --convert-to pdf --outdir /work/out "/fixtures/$name" \
      >/work/convert.log 2>&1
  RC=$?
  ELAPSED=""; RSS_KB=""
  if [ -s /work/time.txt ]; then read -r ELAPSED RSS_KB < /work/time.txt || true; fi
}

is_pdf() { [ -f "/work/out/$1" ] && [ "$(head -c 4 "/work/out/$1")" = "%PDF" ]; }

echo "--- network and read-only checks inside the argv"
netdevs=$(as_verevon bwrap "${ARGV[@]}" sh -c \
  "tail -n +3 /proc/self/net/dev | cut -d: -f1 | tr -d ' '" 2>/dev/null || true)
if [ "$netdevs" = "lo" ]; then ok "network namespace has only lo"; else bad "sandbox network devices: '$netdevs'"; fi

rochk=$(as_verevon bwrap "${ARGV[@]}" sh -c '
  if echo x > /skills/w 2>/dev/null; then echo skills-writable; else echo skills-ro; fi
  if echo x > /usr/w 2>/dev/null; then echo root-writable; else echo root-ro; fi
  if [ -f /skills/SKILL.md ]; then echo skill-visible; fi' 2>/dev/null || true)
echo "$rochk" | grep -qx "skills-ro" && ok "/skills is read-only inside the sandbox" || bad "/skills is writable: $rochk"
echo "$rochk" | grep -qx "root-ro" && ok "root filesystem is read-only inside the sandbox" || bad "root is writable: $rochk"
echo "$rochk" | grep -qx "skill-visible" && ok "the /skills bind shows the staged skill" || bad "/skills bind not visible"

echo "--- good fixtures (cold first, then warm on the same profile)"
PROFILE=/work/lo-profile
[ -e "$PROFILE" ] && bad "profile already exists before the cold run"
# Macros and link updates are off before the first conversion. The cold run
# creates the rest of the profile around this file, so every run starts from
# the same settings. MacroSecurityLevel 3 is the highest level; link update 0
# is the Never choice, set for Writer, Calc and Impress.
MACRO_LEVEL=3
LINK_UPDATE=0
mkdir -p "$PROFILE/user"
cat > "$PROFILE/user/registrymodifications.xcu" <<XCU
<?xml version="1.0" encoding="UTF-8"?>
<oor:items xmlns:oor="http://openoffice.org/2001/registry" xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
<item oor:path="/org.openoffice.Office.Common/Security/Scripting"><prop oor:name="MacroSecurityLevel" oor:op="fuse"><value>$MACRO_LEVEL</value></prop></item>
<item oor:path="/org.openoffice.Office.Writer/Content/Update"><prop oor:name="Link" oor:op="fuse"><value>$LINK_UPDATE</value></prop></item>
<item oor:path="/org.openoffice.Office.Calc/Content/Update"><prop oor:name="Link" oor:op="fuse"><value>$LINK_UPDATE</value></prop></item>
<item oor:path="/org.openoffice.Office.Impress/Content/Update"><prop oor:name="Link" oor:op="fuse"><value>$LINK_UPDATE</value></prop></item>
</oor:items>
XCU
chown -R 10001:10001 "$PROFILE"
info "profile macros: MacroSecurityLevel $MACRO_LEVEL (highest); link updates: $LINK_UPDATE (never) for Writer, Calc, Impress"
COLD_S=""; COLD_KB=""
for name in good_budget.docx good_budget.xlsx good_deck.pptx good_page.pdf; do
  convert "$name" "$PROFILE"
  stem="${name%.*}"
  if [ "$RC" -eq 0 ] && is_pdf "$stem.pdf"; then
    ok "$name converted (${ELAPSED}s, peak RSS ${RSS_KB} KiB)"
  else
    bad "$name did not convert: exit $RC, see /work/convert.log"; tail -n 5 /work/convert.log
  fi
  if [ -z "$COLD_S" ]; then COLD_S="$ELAPSED"; COLD_KB="$RSS_KB"; fi
  [ "$name" = "good_budget.docx" ] && info "cold start $ELAPSED s, peak RSS $RSS_KB KiB"
  [ "$name" != "good_budget.docx" ] && info "warm $name ${ELAPSED}s, peak RSS ${RSS_KB} KiB"
done

echo "--- budget gates"
if awk -v s="${COLD_S:-999}" 'BEGIN { exit !(s <= 25) }'; then
  ok "cold start ${COLD_S}s <= 25s"
else
  bad "cold start ${COLD_S:-n/a}s exceeds 25s"
fi
if [ -n "${COLD_KB:-}" ] && [ "$COLD_KB" -le 1048576 ]; then
  ok "peak RSS ${COLD_KB} KiB <= 1 GiB"
else
  bad "peak RSS ${COLD_KB:-n/a} KiB exceeds 1 GiB"
fi

echo "--- broken fixtures (reported, not gated)"
for name in broken_truncated_zip.xlsx broken_missing_rel_target.docx \
  broken_formula_missing_sheet.xlsx broken_literal_div0.xlsx \
  broken_empty_deck.pptx broken_zero_pages.pdf; do
  convert "$name" "$PROFILE"
  stem="${name%.*}"
  if [ "$RC" -eq 0 ] && is_pdf "$stem.pdf"; then pdf=pdf-produced; else pdf=no-pdf; fi
  info "$name: exit $RC, $pdf, ${ELAPSED:-n/a}s"
done

if [ "$fail" -eq 0 ]; then
  echo "PASS — LibreOffice converts the good corpus under the bwrap argv within budget."
else
  echo "FAIL — see the lines above; D5 falls back to a sidecar or structural-only."
  exit 1
fi
SPIKE
