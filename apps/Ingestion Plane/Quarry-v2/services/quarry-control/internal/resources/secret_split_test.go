package resources

// D2-050: the orchestrator-only internal routes verify with their own secret,
// and finalize refuses a job of another org. The router is built the way
// cmd/control/main.go builds it: httpx.SplitHMAC over the mounted routes.

import (
	"bytes"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"net/http"
	"net/http/httptest"
	"sort"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// Two distinct test vectors, assembled at run time so the literals do not look
// like credentials. NOT real secrets.
func splitEdgeSecret() string         { return strings.Repeat("e0", 16) }
func splitOrchestratorSecret() string { return strings.Repeat("c1", 16) }

// splitServer mounts the jobs, schedules and tombstone routes behind SplitHMAC with
// the two secrets, in enforce mode.
func splitServer(t *testing.T) (http.Handler, store.DB) {
	t.Helper()
	db := store.NewMemory()
	db.ZdrTombstones().SetErasureMarkers(true)
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	r.Group(func(r chi.Router) {
		r.Use(httpx.SplitHMAC(
			httpx.NewHMACVerifier(splitOrchestratorSecret(), true),
			httpx.NewHMACVerifier(splitEdgeSecret(), true),
		))
		MountJobs(r, db)
		MountSchedules(r, db)
		MountZdrTombstones(r, db)
	})
	return r, db
}

var splitNonce int

// splitSigned builds a request signed with secret (the canonical string of httpx/hmac.go).
func splitSigned(secret, method, target string, body []byte) *http.Request {
	splitNonce++
	nonce := "5a5a5a5a5a5a5a5a5a5a5a5a" + strconv.Itoa(10000000+splitNonce)
	ts := time.Now().Unix()
	bodyHash := sha256.Sum256(body)
	canonical := method + "\n" + target + "\n" + hex.EncodeToString(bodyHash[:]) +
		"\n" + strconv.FormatInt(ts, 10) + "\n" + nonce
	mac := hmac.New(sha256.New, []byte(secret))
	_, _ = mac.Write([]byte(canonical))
	req := httptest.NewRequest(method, target, bytes.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set(httpx.HMACHeaderSig, "sig_v1="+base64.StdEncoding.EncodeToString(mac.Sum(nil)))
	req.Header.Set(httpx.HMACHeaderTS, strconv.FormatInt(ts, 10))
	req.Header.Set(httpx.HMACHeaderNonce, nonce)
	return req
}

func splitServe(h http.Handler, req *http.Request) *httptest.ResponseRecorder {
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	return rec
}

// splitSeedRunning stores a running job with a workflow id for org.
func splitSeedRunning(t *testing.T, db store.DB, org string) quarrycontracts.ID {
	t.Helper()
	id := lcSeedJob(t, db, org, "crawl", "running", nil)
	j, ok := db.Jobs().GetByOrg(org, id)
	if !ok {
		t.Fatal("seeded job not found")
	}
	j.WorkflowID = "quarry-job-" + string(id)
	if err := db.Jobs().Update(j); err != nil {
		t.Fatal(err)
	}
	return id
}

// The orchestrator-only routes, as the orchestrator calls them.
func orchestratorRoutes(id quarrycontracts.ID) []struct{ method, target, body string } {
	return []struct{ method, target, body string }{
		{http.MethodPost, "/v1/internal/jobs/claim", `{"limit":1}`},
		{http.MethodGet, "/v1/internal/jobs/pending-control?limit=5", ""},
		{http.MethodPost, "/v1/internal/jobs/" + string(id) + "/control-ack", `{"applied_seq":1}`},
		{http.MethodGet, "/v1/internal/jobs/running?limit=5", ""},
		{http.MethodPost, "/v1/internal/jobs/" + string(id) + "/finalize", `{"status":"cancelled"}`},
		{http.MethodGet, "/v1/internal/schedules/desired", ""},
		{http.MethodPost, "/v1/internal/schedules/sch_x/runs", `{}`},
	}
}

// TestSecretSplit_EdgeSecretRefusedOnOrchestratorRoutes: a request to any of the
// seven orchestrator-only routes signed with the EDGE secret is 401; signed with
// the control-internal secret it passes the verifier (any status but 401).
func TestSecretSplit_EdgeSecretRefusedOnOrchestratorRoutes(t *testing.T) {
	h, db := splitServer(t)
	id := splitSeedRunning(t, db, "org_a")
	for _, rt := range orchestratorRoutes(id) {
		bad := splitServe(h, splitSigned(splitEdgeSecret(), rt.method, rt.target, []byte(rt.body)))
		if bad.Code != http.StatusUnauthorized {
			t.Errorf("%s %s with the edge secret: status %d, want 401", rt.method, rt.target, bad.Code)
		}
		good := splitServe(h, splitSigned(splitOrchestratorSecret(), rt.method, rt.target, []byte(rt.body)))
		if good.Code == http.StatusUnauthorized {
			t.Errorf("%s %s with the control-internal secret: 401 (%s)", rt.method, rt.target, good.Body.String())
		}
	}
}

// TestSecretSplit_UnsignedAndGarbageRefused: no signature, and a signature made
// with a third secret, are both 401 on an orchestrator-only route.
func TestSecretSplit_UnsignedAndGarbageRefused(t *testing.T) {
	h, db := splitServer(t)
	id := splitSeedRunning(t, db, "org_a")
	target := "/v1/internal/jobs/" + string(id) + "/finalize"
	body := []byte(`{"status":"cancelled"}`)

	unsigned := httptest.NewRequest(http.MethodPost, target, bytes.NewReader(body))
	if rec := splitServe(h, unsigned); rec.Code != http.StatusUnauthorized {
		t.Fatalf("unsigned finalize: status %d, want 401", rec.Code)
	}
	if rec := splitServe(h, splitSigned(strings.Repeat("a9", 16), http.MethodPost, target, body)); rec.Code != http.StatusUnauthorized {
		t.Fatalf("finalize signed with a third secret: status %d, want 401", rec.Code)
	}
	j, _ := db.Jobs().Get(id)
	if j.Status != "running" {
		t.Fatalf("job status after refused finalizes = %q, want running", j.Status)
	}
}

// TestSecretSplit_ControlSecretRefusedOnEdgeRoutes: the control-internal secret
// opens nothing on the edge side. /v1/jobs and the edge's ZDR tombstone route
// verify with the edge secret only.
func TestSecretSplit_ControlSecretRefusedOnEdgeRoutes(t *testing.T) {
	h, _ := splitServer(t)
	edgeRoutes := []struct{ method, target, body string }{
		{http.MethodGet, "/v1/jobs/ongoing?org_id=org_a", ""},
		{http.MethodPost, "/v1/internal/zdr-tombstones", `{}`},
	}
	for _, rt := range edgeRoutes {
		bad := splitServe(h, splitSigned(splitOrchestratorSecret(), rt.method, rt.target, []byte(rt.body)))
		if bad.Code != http.StatusUnauthorized {
			t.Errorf("%s %s with the control-internal secret: status %d, want 401", rt.method, rt.target, bad.Code)
		}
		good := splitServe(h, splitSigned(splitEdgeSecret(), rt.method, rt.target, []byte(rt.body)))
		if good.Code == http.StatusUnauthorized {
			t.Errorf("%s %s with the edge secret: 401 (%s)", rt.method, rt.target, good.Body.String())
		}
	}
}

// TestSecretSplit_RouteTableMatchesGuard: every /v1/internal/ route the control
// mounts is either in the orchestrator-only group or is the edge's tombstone
// route. A new internal route cannot slip into the wrong group unnoticed.
func TestSecretSplit_RouteTableMatchesGuard(t *testing.T) {
	db := store.NewMemory()
	r := chi.NewRouter()
	MountJobs(r, db)
	MountSchedules(r, db)
	MountZdrTombstones(r, db)

	var internal []string
	err := chi.Walk(r, func(method, route string, _ http.Handler, _ ...func(http.Handler) http.Handler) error {
		if strings.HasPrefix(route, "/v1/internal/") {
			internal = append(internal, method+" "+route)
			orchestrator := httpx.IsOrchestratorOnlyPath(route)
			edge := route == "/v1/internal/zdr-tombstones"
			if orchestrator == edge {
				t.Errorf("%s %s: orchestrator-only=%v, edge-only=%v; exactly one must hold", method, route, orchestrator, edge)
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	sort.Strings(internal)
	if len(internal) != 8 {
		t.Fatalf("control mounts %d /v1/internal/ routes, want 8: %v", len(internal), internal)
	}
}

// TestFinalize_OtherOrgRefused: finalize that names another org answers 404, like
// a missing job, and the job is untouched; the owning org's finalize ends it.
func TestFinalize_OtherOrgRefused(t *testing.T) {
	h, db := splitServer(t)
	id := splitSeedRunning(t, db, "org_a")
	target := "/v1/internal/jobs/" + string(id) + "/finalize"

	other := splitServe(h, splitSigned(splitOrchestratorSecret(), http.MethodPost, target,
		[]byte(`{"status":"cancelled","forced":true,"org_id":"org_b"}`)))
	if other.Code != http.StatusNotFound {
		t.Fatalf("finalize naming another org: status %d, want 404 (%s)", other.Code, other.Body.String())
	}
	j, _ := db.Jobs().Get(id)
	if j.Status != "running" {
		t.Fatalf("job of org_a after org_b's finalize = %q, want running", j.Status)
	}

	byQuery := splitServe(h, splitSigned(splitOrchestratorSecret(), http.MethodPost, target+"?org_id=org_b",
		[]byte(`{"status":"cancelled"}`)))
	if byQuery.Code != http.StatusNotFound {
		t.Fatalf("finalize naming another org by query: status %d, want 404", byQuery.Code)
	}

	own := splitServe(h, splitSigned(splitOrchestratorSecret(), http.MethodPost, target,
		[]byte(`{"status":"cancelled","forced":true,"org_id":"org_a"}`)))
	if own.Code != http.StatusOK || !strings.Contains(own.Body.String(), `"changed":true`) {
		t.Fatalf("finalize by the owning org: status %d, body %s", own.Code, own.Body.String())
	}
	j, _ = db.Jobs().Get(id)
	if j.Status != "cancelled" {
		t.Fatalf("job status after the owner's finalize = %q, want cancelled", j.Status)
	}
}

// TestFinalize_OrgIDOptionalByDefaultRequiredByFlag: with no org_id a finalize is
// still accepted (an orchestrator that has not been redeployed keeps working); with
// QUARRY_CONTROL__FINALIZE_ORG_REQUIRED on it is 400 and changes nothing.
func TestFinalize_OrgIDOptionalByDefaultRequiredByFlag(t *testing.T) {
	h, db := lcRouter(t)
	first := splitSeedRunning(t, db, "org_a")
	second := splitSeedRunning(t, db, "org_a")

	rec := lcDo(h, http.MethodPost, "/v1/internal/jobs/"+string(first)+"/finalize", map[string]any{"status": "cancelled"})
	if rec.Code != http.StatusOK {
		t.Fatalf("finalize with no org_id, flag off: status %d, want 200", rec.Code)
	}

	t.Setenv("QUARRY_CONTROL__FINALIZE_ORG_REQUIRED", "1")
	rec = lcDo(h, http.MethodPost, "/v1/internal/jobs/"+string(second)+"/finalize", map[string]any{"status": "cancelled"})
	if rec.Code != http.StatusBadRequest {
		t.Fatalf("finalize with no org_id, flag on: status %d, want 400", rec.Code)
	}
	if j, _ := db.Jobs().Get(second); j.Status != "running" {
		t.Fatalf("job status after the refused finalize = %q, want running", j.Status)
	}
	rec = lcDo(h, http.MethodPost, "/v1/internal/jobs/"+string(second)+"/finalize", map[string]any{"status": "cancelled", "org_id": "org_a"})
	if rec.Code != http.StatusOK {
		t.Fatalf("finalize with org_id, flag on: status %d, want 200", rec.Code)
	}
}
