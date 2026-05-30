# Onboarding E2E & Analytics — Quick Start

**⏱️ 5-minute setup · 10-minute test · instant results**

---

## TL;DR — Run the Test

```bash
# 1. Go to project root
cd /Volumes/Lagring/Triodelab/CoreSystem

# 2. Make script executable
chmod +x scripts/e2e-onboarding-test.sh

# 3. Run full test suite
scripts/e2e-onboarding-test.sh

# 4. View results
cat /tmp/onboarding-e2e-results-*.json | jq .
```

**That's it.** Results printed to console + saved to `/tmp/` as JSON.

---

## What Gets Tested

```
✅ OAuth Sign-In & user provisioning
✅ Profile completion (name, timezone, job)
✅ Organization creation
✅ Website crawl via Quarry (with SSE streaming)
✅ Team member invitations (3 people + roles)
✅ Onboarding completion marker
✅ Data Plane document ingestion
✅ AI workspace + retrieval queries
✅ Tenant isolation (no cross-org leakage)
```

---

## Understanding Results

### Console Output
```
═══════════════════════════════════════════════════════════════
  ONBOARDING E2E TEST RESULTS
═══════════════════════════════════════════════════════════════
  Status: ✅ ALL PASS
  Passed: 9 / 9
  Duration: 45231ms
═══════════════════════════════════════════════════════════════

Step Timings:
  oauth_signin              :   234ms
  profile_completion        :   512ms
  org_creation              :   189ms
  website_config            :  8923ms  (includes crawl monitoring)
  team_invitation           :   623ms
  onboarding_complete       :   267ms
  data_plane_sync           :  2341ms
  ai_workspace_access       :  1523ms
  tenant_isolation          :   234ms
```

**Reading the output:**
- `Status: ✅ ALL PASS` = All 9 steps succeeded
- `Passed: 9 / 9` = 100% success rate
- `Duration: 45231ms` = Total time for all steps
- Individual step times for performance baseline

### JSON Results File
```bash
# File location:
/tmp/onboarding-e2e-results-{TIMESTAMP}.json

# View with jq:
cat /tmp/onboarding-e2e-results-*.json | jq '.step_results'

# Pretty print full results:
jq . /tmp/onboarding-e2e-results-*.json
```

**JSON structure:**
```json
{
  "test_run_id": "onboarding-e2e-1709049600",
  "status_overall": "PASS",
  "passed_steps": 9,
  "failed_steps": 0,
  "total_duration_ms": 45231,
  "step_results": {
    "oauth_signin": "PASS",
    "profile_completion": "PASS",
    ...
  }
}
```

---

## Analytics During Manual Testing

### In Browser Console
```javascript
// After completing onboarding flow:
analyticsService.printSummary()

// Get all metrics
analyticsService.getMetrics()

// View completion rate
const m = analyticsService.getMetrics()
console.log(`Completion: ${(m.completion_rate * 100).toFixed(1)}%`)

// Drop-off analysis
console.table(m.drop_off_by_step)

// Step timing analysis
console.table(m.average_time_per_step)
```

**Output example:**
```
=== Onboarding Analytics Summary ===

Flow Metrics:
  Total Starts:        42
  Completions:        35
  Completion Rate:     83.3%

Top Steps by Time:
  website:   12542ms   (waiting for crawl)
  team:      4231ms    (inviting 3 people)
  org:       2015ms    (org lookup + creation)
  profile:    512ms    (form entry)
  complete:   289ms    (marking flag)

Drop-off Points:
  profile:    2 users   (4.8%)
  org:        1 user    (2.4%)
  website:    1 user    (2.4%)
  team:       3 users   (7.1% - normal, optional)
  complete:   0 users   (0% - all reach final step)

Error Patterns:
  org:       "name already exists" (1)
  website:   "URL unreachable" (1)
```

### Export Analytics
```javascript
// Get all events
const data = analyticsService.exportAnalytics()

// Save to clipboard / download
console.log(JSON.stringify(data, null, 2))

// Send to backend (if endpoint implemented)
await analyticsService.syncToBackend('/api/analytics/onboarding')
```

---

## Troubleshooting

| Issue | Solution |
|-------|----------|
| **Test fails at Step 0** | Services not running: `docker-compose up -d` |
| **OAuth fails** | Check auth-service logs: `docker-compose logs auth-service` |
| **Website crawl timeout** | Website unreachable or Quarry down, check logs |
| **Analytics not recording** | Check browser console for errors, verify localStorage available |
| **Tenant isolation fails** | Check org_id filtering in retrieval service queries |

---

## Performance Targets

**Expected timing (per step):**
- OAuth: 200-400ms
- Profile: 400-700ms
- Org: 150-300ms
- Website: 10-15s (SSE stream wait)
- Team: 1-2s (3 invites)
- Complete: 200-400ms

**Total flow:** 12-20 seconds

If significantly slower, check:
- Network latency
- Backend service logs
- Database query plans

---

## Files Reference

| File | Purpose |
|------|---------|
| `/scripts/e2e-onboarding-test.sh` | Automated test script (450+ lines) |
| `/lib/services/analytics-service.ts` | Analytics tracking service (400+ lines) |
| `/components/onboarding/services/onboarding-service.ts` | Onboarding orchestration (analytics integrated) |
| `/docs/ONBOARDING_E2E_TESTING.md` | Full documentation (this guide) |

---

## Next Steps

1. ✅ **Run test:** `scripts/e2e-onboarding-test.sh`
2. 📊 **Review metrics:** `analyticsService.printSummary()`
3. 🔍 **Identify bottlenecks:** Check step timing data
4. 🐛 **Fix issues:** Review error patterns
5. 📈 **Track progress:** Run tests regularly to monitor improvements

---

## One-Liners

```bash
# Run full test and view results
bash scripts/e2e-onboarding-test.sh && jq . /tmp/onboarding-e2e-results-*.json

# Run test and extract just the step timings
bash scripts/e2e-onboarding-test.sh | grep -A 20 "Step Timings"

# Check if all steps passed
bash scripts/e2e-onboarding-test.sh 2>&1 | grep "Status: ✅"
```

---

**Last Updated:** 28 Feb 2026  
**Status:** All systems GREEN  
**Coverage:** 9/9 steps tested
