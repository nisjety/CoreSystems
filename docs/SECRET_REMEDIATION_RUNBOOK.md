# Secret Remediation Runbook

**Incident date:** 2026-06-18 (found while drafting GDPR/compliance docs)
**Repo state:** local-only (no git remote). Leak originates largely from commit `1eecf8d0` (2026-05-30), present on `main` + 3 `feat/*` branches.
**Policy:** `credential_or_secret` — never persist secrets in cleartext. This runbook contains **no secret values** (only key names, file paths, and fingerprints = `sha256[:6]`).

> ⚠️ Do not commit any real secret value to any file (tracked or not), and do not paste values into this runbook. New values belong only in gitignored `.env` files or a secret manager.

---

## 0. Status snapshot

### ✅ Done (2026-06-18)
- **Azure OpenAI key** (`core-ai-rg` KEY1, RG `Core-Ai-RG`): zero-downtime rotation — configs switched to KEY2, KEY1 regenerated; old value verified dead.
- **Control-plane DB password** (`aquatiq`@controlplane-postgres): `ALTER USER` to a new password; 10 gitignored env files updated; new verified, old rejected (scram path).
- **`BETTER_AUTH_SECRET`**: new shared secret in `auth-core/.env`, `auth-core/.env.docker`, root `.env`; separate prod secret in `auth-core/.env.production`; `docker-compose.yml` L22 changed to required `${BETTER_AUTH_SECRET:?…}`.
- Working tree redacted in 9 tracked files; `.gitignore` confirmed comprehensive (only `*.env.example`/templates tracked).

### ⚠️ Immediate (post-rotation restart — REQUIRED)
```bash
cd "apps/Control Plane" && docker compose up -d --build   # new aquatiq pw + BETTER_AUTH_SECRET
docker compose up -d                                       # repo root: frontend/root stack
```
- Logs all users out (new auth secret) — expected.
- Rebuild Go binaries with stale compiled-in DB pw: `apps/Control Plane/org-core/{server,org-core}`.

### ⏳ Pending (this runbook)
1. Systemic internal-key rotation (§1–§3).
2. Git history scrub before any first push (§4).
3. Prevention controls (§5).

---

## 1. Systemic finding — live internal keys are hardcoded & committed

The gitleaks backstop (full history) found that **live cross-service keys are committed in source, compose, and docs** and are the values in active use. These were **not** auto-remediated (rotation breaks all inter-service auth until every service restarts).

| Key (fingerprint) | Live footprint | Committed at HEAD in (de-hardcode these) |
|---|---|---|
| `INTERNAL_API_KEY` (`651696`, 64ch) | 19 gitignored env files (all planes) | `apps/Control Plane/session-core/internal/internalkey/assert_test.go`, `apps/Frontend Plane/verevon/verevon-gap.md`, `apps/Model Plane v2/docker-compose.yml` |
| `x-internal-api-key` (`b67177`, 33ch) | 2 env files | `apps/Control Plane/docker-compose.yml`, `apps/Frontend Plane/verevonv2/docker-compose.yml`, `apps/Ingestion Plane/docker-compose.yml`, `apps/Control Plane/auth-core/scripts/test-nats-direct.ts`, `apps/Ingestion Plane/smoke-test-integration-api.sh`, `pprof-profile.sh`, `apps/Control Plane/docs/USER_PROFILE_500_ERROR_ANALYSIS.md`, `apps/Frontend Plane/verevon/src/components/auth/AUTH_ARCHITECTURE.md`, `docs/FRONTEND_AUTH_UPDATE_SUMMARY.md` |
| internal key (`88ad0c`, 18ch) | 1 env file | `apps/Ingestion Plane/integration-corev2/internal/{auth/middleware.go,config/config.go,api/server_test.go,handoff/integration.go,handoff/integration_test.go}`, `.../integration-corev2/.env.example`, `.../README.md`, `apps/Application Plane/social-core/internal/integration/client.go`, `apps/Ingestion Plane/docker-compose.yml`, `finspo-core` source/docs |
| keys `069012`, `b2fe6f`, `3610ce` | 1 env file each | various `docker-compose.yml`, `scripts/*.sh/.py`, docs |

**Other likely-real committed secrets to verify & rotate:**
- `GOOGLE_CLIENT_SECRET` (`a37426`) in `apps/Control Plane/docs/SSO_SETUP_GUIDE.md` → if real, rotate in Google Cloud Console (OAuth client) and redact.
- `QUARRY_API_KEY` (`2dfb13`) in `apps/Ingestion Plane/Quarry/configs/.env.example`; `FINSPO_API_KEY` (`7d6ed0`) in `apps/Ingestion Plane/finspo-core/.env.example` → templates should hold placeholders, not real keys.
- credential (`3a56b2`) in `apps/Control Plane/auth-core/scripts/add-password-to-user.js`.

**Likely benign (verify, no rotation expected):** test fixtures — `apps/Model Plane/rust/services/execution-core/tests/scrub_checkpoint_test.rs` (JWT), `apps/Control Plane/user-core/internal/grpc/server_auth_test.go`, `apps/Model Plane/rust/services/execution-core/src/scrub.rs`.

---

## 2. Rotation procedure (per internal key)

Do this **per key**, in a maintenance window (inter-service calls fail between the rotation and the full restart).

**2.1 — Discover every live consumer** (gitignored env files holding the OLD value):
```bash
cd "<repo root>"
OLD='<old-value-from-a-gitignored-.env>'          # read from a live .env; do NOT echo it
grep -rIl --include='.env*' -F "$OLD" . | grep -vE '\.example$|\.template$'
```

**2.2 — Generate a new value** (URL-safe so it works inside DATABASE_URL-style strings):
```bash
NEW=$(openssl rand -hex 32)     # 64 hex chars; for non-URL secrets `openssl rand -base64 32` is fine
```

**2.3 — Update all live env files** (literal replace, value never printed):
```bash
export OLD NEW
python3 - <<'PY'
import os
old=os.environ['OLD']; new=os.environ['NEW']; ROOT=os.getcwd()
SKIP={'.git','node_modules','target','dist','build','.next','.venv','__pycache__'}
def live(b): return ('.env' in b) and not b.endswith(('.example','.template'))
n=0
for dp,dns,fns in os.walk(ROOT):
    dns[:]=[d for d in dns if d not in SKIP]
    for fn in fns:
        if not live(fn): continue
        p=os.path.join(dp,fn); t=open(p,encoding='utf-8',errors='replace').read()
        if old in t: open(p,'w').write(t.replace(old,new)); n+=1; print("updated", os.path.relpath(p,ROOT))
print("files:", n)
PY
```

**2.4 — De-hardcode source/compose/docs** (the "Committed at HEAD in" column, §1). Replace literals with env reads — **no secret fallback**:
- Compose: `KEY: ${INTERNAL_API_KEY:?set in .env}`
- Go: `getEnv("INTERNAL_API_KEY", "")` (empty default; fail fast if unset) — never a literal default.
- Tests: use an obviously-fake fixture (`"test-internal-key-fixture"`), not the real value.
- Docs/scripts: replace with `${INTERNAL_API_KEY}` / `<redacted>`.

**2.5 — Restart order** (control plane owns identity; bring it up first):
```
Control Plane → Application Plane → Data Plane v2 → Model Plane → Ingestion Plane → Frontend
```

**2.6 — Verify:** no working-tree literal remains, and services authenticate:
```bash
git grep -nI -F "$OLD" -- . ; echo "exit $? (1 = clean)"     # must find nothing in tracked files
# functional: hit an inter-service endpoint and confirm 200, not 401
```

---

## 3. Special cases
- **`GOOGLE_CLIENT_SECRET`** — rotate in Google Cloud Console → OAuth 2.0 Client; update gitignored env; redact `SSO_SETUP_GUIDE.md`.
- **`.env.example` templates** (`Quarry`, `finspo-core`, root, Control Plane) — replace any real value with `<openssl rand -base64 32>` / `your-…` placeholder. Templates must never carry real values.
- **`apps/Control Plane/org-core/{server,org-core}`** — rebuild (old DB pw compiled in).
- **`auth-core/.env.production` & other `.env.production`** — move production secrets out of laptop files into a real secret manager (Azure Key Vault / Doppler / SOPS-encrypted).

---

## 4. Git history scrub (do AFTER all rotations, BEFORE any first push)

The repo has large uncommitted WIP and no remote. Order:

```bash
# 1. Commit (or stash) the redaction + de-hardcode changes so the tree is clean.
git add -A && git commit -m "chore(security): redact committed secrets; require env-provided values"

# 2. Build a replacements file OUTSIDE the repo (filled from a secure note of the now-DEAD values).
#    Format, one per line — NEVER commit this file:
cat > /tmp/scrub-replacements.txt <<'EOF'
<dead-azure-key>==>***REMOVED***
<dead-control-plane-db-pw>==>***REMOVED***
<stale-db-pw-8rSqS…>==>***REMOVED***
<old-better-auth-secret>==>***REMOVED***
<old-INTERNAL_API_KEY>==>***REMOVED***
<old-x-internal-api-key>==>***REMOVED***
# …one line per confirmed leaked value (all already rotated/dead)…
EOF

# 3. Install + run git-filter-repo across ALL branches (rewrites every commit; hashes change).
pip install git-filter-repo        # or: brew install git-filter-repo
git filter-repo --replace-text /tmp/scrub-replacements.txt --force

# 4. Shred the replacements file.
shred -u /tmp/scrub-replacements.txt

# 5. Verify history is clean (expect no output):
for v in <fingerprint-checks>; do git log --all -S"$v" --oneline; done
git reflog expire --expire=now --all && git gc --prune=now --aggressive
```
> If a remote is ever added later, this scrub must happen **before** the first push, and any clone/backup made before the scrub still contains the secrets — rotation (already done) is the real protection.

---

## 5. Prevention (close the recurrence)
1. **Pre-commit hook** — block commits containing secrets:
   ```bash
   # .pre-commit-config.yaml → gitleaks hook, or .git/hooks/pre-commit:
   gitleaks protect --staged --redact --verbose
   ```
2. **CI gate** — `gitleaks detect --redact` on every PR; fail the build on findings.
3. **Templates discipline** — `.env.example`/`*.template` may contain ONLY placeholders; add a CI check that diffs template values against live-env fingerprints.
4. **No secrets in source/compose** — defaults must be empty + fail-fast (`${VAR:?}` / `getEnv(k,"")`), never a real literal.
5. **Production secrets** in a manager (Azure Key Vault, etc.), not `.env.production` files on dev machines.
6. **Rotate on schedule** — internal service keys quarterly; document owners per key.

---

## Appendix — fingerprints (sha256[:6], for matching; not the values)
Azure `ef7b3f` · ctrl-plane DB pw `03e24b` · stale DB pw `1222c1` · BETTER_AUTH_SECRET `fd6148` · INTERNAL_API_KEY `651696` · x-internal-api-key `b67177` · internal `88ad0c`/`069012`/`b2fe6f`/`3610ce` · GOOGLE_CLIENT_SECRET `a37426` · QUARRY_API_KEY `2dfb13` · FINSPO_API_KEY `7d6ed0`.
