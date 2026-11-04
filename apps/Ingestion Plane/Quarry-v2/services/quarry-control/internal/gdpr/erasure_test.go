package gdpr

import (
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/rs/zerolog"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// fakePurger is an in-test OrgPurger: it records every orgID it was asked
// to purge and returns a canned PurgeResult, so HandleErasure is testable
// with no real store or NATS wiring — mirroring documents-api-go's
// fakeRepository pattern for its own HandleErasure test.
type fakePurger struct {
	calls  []string
	result store.PurgeResult
	err    error
}

func (f *fakePurger) PurgeOrg(orgID string) (store.PurgeResult, error) {
	f.calls = append(f.calls, orgID)
	return f.result, f.err
}

func orgErasurePayload(t *testing.T, orgID, requestedBy string) []byte {
	t.Helper()
	// Mirrors org-core's PublishGDPRErasureFanout payload exactly:
	// apps/Control Plane/org-core/internal/org/gdpr_erasure_fanout.go.
	body, err := json.Marshal(map[string]any{
		"subject_type": "organization",
		"subject_id":   orgID,
		"org_id":       orgID,
		"requested_by": requestedBy,
		"ts":           "2026-07-20T12:00:00.000000000Z",
	})
	if err != nil {
		t.Fatalf("marshal fixture payload: %v", err)
	}
	return body
}

func TestHandleErasure_OrganizationSubjectPurgesTheOrg(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{result: store.PurgeResult{JobsDeleted: 3, SourcesDeleted: 1}}
	payload := orgErasurePayload(t, "org_a", "system:org-core-purge-cron")

	result, err := HandleErasure(purger, payload)
	if err != nil {
		t.Fatalf("HandleErasure: %v", err)
	}
	if len(purger.calls) != 1 || purger.calls[0] != "org_a" {
		t.Fatalf("purger.calls=%v want=[org_a]", purger.calls)
	}
	if result != purger.result {
		t.Errorf("result=%+v want=%+v", result, purger.result)
	}
}

// TestHandleErasure_UserSubjectIsNoOp proves a "user"/"user_anonymize"
// erasure event — the SAME subject, published by user-core for a different
// purpose — is ignored rather than mishandled as an org purge. Quarry
// control has no per-user data model; only "organization" is its concern.
func TestHandleErasure_UserSubjectIsNoOp(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{result: store.PurgeResult{JobsDeleted: 99}}
	payload, err := json.Marshal(map[string]any{
		"event_id":     "evt_123",
		"subject_type": "user",
		"subject_id":   "user_a",
		"org_id":       "org_a",
		"requested_by": "user_a",
		"mode":         "hard",
		"ts":           "2026-07-20T12:00:00.000000000Z",
	})
	if err != nil {
		t.Fatalf("marshal fixture: %v", err)
	}

	result, err := HandleErasure(purger, payload)
	if err != nil {
		t.Fatalf("HandleErasure: %v", err)
	}
	if len(purger.calls) != 0 {
		t.Fatalf("purger.calls=%v want=[] — a user-subject event must never trigger an org purge", purger.calls)
	}
	if result.Total() != 0 {
		t.Errorf("result.Total()=%d want=0", result.Total())
	}
}

func TestHandleErasure_MissingOrgIDIsNoOp(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{}
	payload := orgErasurePayload(t, "", "actor")

	result, err := HandleErasure(purger, payload)
	if err != nil {
		t.Fatalf("HandleErasure: %v", err)
	}
	if len(purger.calls) != 0 {
		t.Fatalf("purger.calls=%v want=[] — a blank org_id must never be forwarded to PurgeOrg", purger.calls)
	}
	if result.Total() != 0 {
		t.Errorf("result.Total()=%d want=0", result.Total())
	}
}

func TestHandleErasure_MalformedPayloadReturnsError(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{}
	_, err := HandleErasure(purger, []byte("not json"))
	if err == nil {
		t.Fatal("HandleErasure(malformed payload) = nil error, want an error")
	}
	// Must be classified as poison (subscriber.go acks immediately on this,
	// never retries), not treated the same as a transient store failure.
	var poison *poisonEventError
	if !errors.As(err, &poison) {
		t.Errorf("err=%v (%T) want a *poisonEventError so the subscriber acks instead of retrying", err, err)
	}
	if len(purger.calls) != 0 {
		t.Fatalf("purger.calls=%v want=[] on a decode failure", purger.calls)
	}
}

// TestHandleErasure_OrgIDFallsBackToSubjectID mirrors
// conversation-core-go's org-erasure consumer's defensive fallback for this
// exact event: org-core's real publisher always sets both org_id and
// subject_id to the same value, but a payload with org_id blank and only
// subject_id populated must still resolve to a purge, not a silent no-op.
func TestHandleErasure_OrgIDFallsBackToSubjectID(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{result: store.PurgeResult{JobsDeleted: 1}}
	payload, err := json.Marshal(map[string]any{
		"subject_type": "organization",
		"subject_id":   "org_a",
		"org_id":       "",
		"requested_by": "system:org-core-purge-cron",
		"ts":           "2026-07-20T12:00:00.000000000Z",
	})
	if err != nil {
		t.Fatalf("marshal fixture: %v", err)
	}

	if _, err := HandleErasure(purger, payload); err != nil {
		t.Fatalf("HandleErasure: %v", err)
	}
	if len(purger.calls) != 1 || purger.calls[0] != "org_a" {
		t.Fatalf("purger.calls=%v want=[org_a]", purger.calls)
	}
}

// TestHandleErasure_IsIdempotentAcrossRedelivery mirrors the fixed
// contract's requirement directly: NATS is at-least-once, so the same
// erasure event can be delivered twice. Calling HandleErasure twice must
// purge twice without erroring — the underlying store makes the SECOND
// call a no-op (see store.TestMemoryPurgeOrg_IsIdempotent), but this proves
// the gdpr package's own no-op/organization-subject gating never turns a
// legitimate redelivery into an error either.
func TestHandleErasure_IsIdempotentAcrossRedelivery(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{result: store.PurgeResult{JobsDeleted: 1}}
	payload := orgErasurePayload(t, "org_a", "system:org-core-purge-cron")

	if _, err := HandleErasure(purger, payload); err != nil {
		t.Fatalf("first HandleErasure: %v", err)
	}
	if _, err := HandleErasure(purger, payload); err != nil {
		t.Fatalf("second HandleErasure (redelivery): %v", err)
	}
	if len(purger.calls) != 2 || purger.calls[0] != "org_a" || purger.calls[1] != "org_a" {
		t.Fatalf("purger.calls=%v want=[org_a org_a]", purger.calls)
	}
}

// fakeEdge is an in-test EdgePurger: it records every org it was asked to purge
// in quarry-edge and returns a canned result or error (WS17.E3).
type fakeEdge struct {
	calls  []string
	result EdgePurgeResult
	err    error
}

func (f *fakeEdge) PurgeEdgeOrg(orgID string) (EdgePurgeResult, error) {
	f.calls = append(f.calls, orgID)
	return f.result, f.err
}

func TestHandleErasureWithEdge_PurgesStoreThenEdge(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{result: store.PurgeResult{JobsDeleted: 2}}
	edge := &fakeEdge{result: EdgePurgeResult{Families: []EdgePurgeFamily{{Family: "profiles", RowsDeleted: 3, TombstonesDeleted: 1, CacheKeysDeleted: 4}}}}
	payload := orgErasurePayload(t, "org_a", "system:org-core-purge-cron")

	result, edgeResult, err := HandleErasureWithEdge(purger, edge, payload)
	if err != nil {
		t.Fatalf("HandleErasureWithEdge: %v", err)
	}
	if len(purger.calls) != 1 || purger.calls[0] != "org_a" {
		t.Fatalf("store purger calls=%v want=[org_a]", purger.calls)
	}
	if len(edge.calls) != 1 || edge.calls[0] != "org_a" {
		t.Fatalf("edge calls=%v want=[org_a]", edge.calls)
	}
	if result != purger.result {
		t.Errorf("store result=%+v want=%+v", result, purger.result)
	}
	if edgeResult.Total() != 8 {
		t.Errorf("edge Total()=%d want=8", edgeResult.Total())
	}
}

func TestHandleErasureWithEdge_NilEdgeIsTheStoreOnly(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{}
	payload := orgErasurePayload(t, "org_a", "actor")

	if _, edgeResult, err := HandleErasureWithEdge(purger, nil, payload); err != nil {
		t.Fatalf("HandleErasureWithEdge(nil edge): %v", err)
	} else if edgeResult.Total() != 0 {
		t.Errorf("edge Total()=%d want=0 when the hop is off", edgeResult.Total())
	}
	if len(purger.calls) != 1 {
		t.Fatalf("store calls=%v want one", purger.calls)
	}
}

// An edge failure is an error, not a poison event: the subscriber must NAK it so
// JetStream redelivers (a transient edge outage must not drop the erasure).
func TestHandleErasureWithEdge_EdgeFailureIsRetryableNotPoison(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{}
	edge := &fakeEdge{err: errors.New("gdpr: edge purge call failed")}
	payload := orgErasurePayload(t, "org_a", "actor")

	_, _, err := HandleErasureWithEdge(purger, edge, payload)
	if err == nil {
		t.Fatal("edge failure returned nil error; the erasure would be acked and lost")
	}
	var poison *poisonEventError
	if errors.As(err, &poison) {
		t.Fatalf("edge failure classified as poison (%v); it must be retried", err)
	}
}

func TestHandleErasureWithEdge_UserSubjectNeverReachesTheEdge(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{}
	edge := &fakeEdge{}
	payload, err := json.Marshal(map[string]any{
		"subject_type": "user",
		"subject_id":   "user_a",
		"org_id":       "org_a",
	})
	if err != nil {
		t.Fatalf("marshal fixture: %v", err)
	}
	if _, _, err := HandleErasureWithEdge(purger, edge, payload); err != nil {
		t.Fatalf("HandleErasureWithEdge: %v", err)
	}
	if len(edge.calls) != 0 {
		t.Fatalf("edge calls=%v want none for a user subject", edge.calls)
	}
}

// fakeUsers is an in-test UserReceiptPurger (W2-U49).
type fakeUsers struct {
	calls  [][2]string
	result EdgePurgeResult
	err    error
}

func (f *fakeUsers) PurgeEdgeUserReceipts(orgID, userID string) (EdgePurgeResult, error) {
	f.calls = append(f.calls, [2]string{orgID, userID})
	return f.result, f.err
}

// userErasurePayload mirrors user-core's erasureFanoutPayload
// (apps/Control Plane/user-core/internal/users/gdpr_erasure_saga.go).
func userErasurePayload(t *testing.T, subjectType, userID, orgID string) []byte {
	t.Helper()
	body, err := json.Marshal(map[string]any{
		"event_id":     "op_1:" + orgID,
		"operation_id": "op_1",
		"subject_type": subjectType,
		"subject_id":   userID,
		"org_id":       orgID,
		"requested_by": "admin_a",
		"mode":         "hard",
		"ts":           "2026-11-03T12:00:00.000000000Z",
	})
	if err != nil {
		t.Fatalf("marshal fixture payload: %v", err)
	}
	return body
}

func TestHandleErasureWithUsers_UserSubjectDeletesThatUsersReceipts(t *testing.T) {
	t.Parallel()
	for _, subjectType := range []string{"user", "user_anonymize"} {
		purger := &fakePurger{}
		edge := &fakeEdge{}
		users := &fakeUsers{result: EdgePurgeResult{Families: []EdgePurgeFamily{{Family: "step_receipts", RowsDeleted: 3}}}}
		payload := userErasurePayload(t, subjectType, "user_a", "org_a")

		_, edgeResult, err := HandleErasureWithUsers(purger, edge, users, payload)
		if err != nil {
			t.Fatalf("%s: HandleErasureWithUsers: %v", subjectType, err)
		}
		if len(users.calls) != 1 || users.calls[0] != [2]string{"org_a", "user_a"} {
			t.Fatalf("%s: user hop calls=%v want=[[org_a user_a]]", subjectType, users.calls)
		}
		if len(purger.calls) != 0 || len(edge.calls) != 0 {
			t.Fatalf("%s: a user event reached the org purge (store=%v edge=%v)", subjectType, purger.calls, edge.calls)
		}
		if edgeResult.Total() != 3 {
			t.Errorf("%s: Total()=%d want=3", subjectType, edgeResult.Total())
		}
	}
}

func TestHandleErasureWithUsers_FlagOffKeepsUserEventsANoOp(t *testing.T) {
	t.Parallel()
	edge := &fakeEdge{}
	payload := userErasurePayload(t, "user", "user_a", "org_a")
	if _, res, err := HandleErasureWithUsers(&fakePurger{}, edge, nil, payload); err != nil || res.Total() != 0 {
		t.Fatalf("nil user hop: res=%+v err=%v want a no-op", res, err)
	}
	if len(edge.calls) != 0 {
		t.Fatalf("edge calls=%v want none", edge.calls)
	}
}

func TestHandleErasureWithUsers_MissingIdsAreANoOp(t *testing.T) {
	t.Parallel()
	users := &fakeUsers{}
	for _, payload := range [][]byte{
		userErasurePayload(t, "user", "", "org_a"),
		userErasurePayload(t, "user", "user_a", ""),
	} {
		if _, _, err := HandleErasureWithUsers(&fakePurger{}, nil, users, payload); err != nil {
			t.Fatalf("HandleErasureWithUsers: %v", err)
		}
	}
	if len(users.calls) != 0 {
		t.Fatalf("user hop calls=%v want none when an id is missing", users.calls)
	}
}

func TestHandleErasureWithUsers_HopFailureIsRetryableNotPoison(t *testing.T) {
	t.Parallel()
	users := &fakeUsers{err: errors.New("gdpr: edge purge call failed")}
	payload := userErasurePayload(t, "user", "user_a", "org_a")
	_, _, err := HandleErasureWithUsers(&fakePurger{}, nil, users, payload)
	if err == nil {
		t.Fatal("hop failure returned nil error; the erasure would be acked and lost")
	}
	var poison *poisonEventError
	if errors.As(err, &poison) {
		t.Fatalf("hop failure classified as poison (%v); it must be retried", err)
	}
}

// A redelivery repeats the same idempotent call; zeros the second time are fine.
func TestHandleErasureWithUsers_RedeliveryRepeatsTheIdempotentCall(t *testing.T) {
	t.Parallel()
	users := &fakeUsers{}
	payload := userErasurePayload(t, "user", "user_a", "org_a")
	for i := 0; i < 2; i++ {
		if _, _, err := HandleErasureWithUsers(&fakePurger{}, nil, users, payload); err != nil {
			t.Fatalf("delivery %d: %v", i+1, err)
		}
	}
	if len(users.calls) != 2 {
		t.Fatalf("user hop calls=%d want=2", len(users.calls))
	}
}

func TestHandleErasureWithUsers_OrganizationEventIsUnchanged(t *testing.T) {
	t.Parallel()
	purger := &fakePurger{result: store.PurgeResult{JobsDeleted: 1}}
	users := &fakeUsers{}
	payload := orgErasurePayload(t, "org_a", "system:org-core-purge-cron")
	if _, _, err := HandleErasureWithUsers(purger, nil, users, payload); err != nil {
		t.Fatalf("HandleErasureWithUsers: %v", err)
	}
	if len(purger.calls) != 1 || len(users.calls) != 0 {
		t.Fatalf("store calls=%v user calls=%v want the org purge only", purger.calls, users.calls)
	}
}

func TestUserReceiptPurgerFromEnv_OffByDefault(t *testing.T) {
	t.Parallel()
	edge := NewHTTPEdgePurger("http://edge", "tok", nil)
	for _, raw := range []string{"", "off", "0", "false"} {
		got := UserReceiptPurgerFromEnv(func(string) string { return raw }, edge, zerolog.Nop())
		if got != nil {
			t.Fatalf("flag %q built a user hop", raw)
		}
	}
}

func TestUserReceiptPurgerFromEnv_OnUsesTheEdgeHop(t *testing.T) {
	t.Parallel()
	edge := NewHTTPEdgePurger("http://edge", "tok", nil)
	got := UserReceiptPurgerFromEnv(func(k string) string {
		if k == UserReceiptsFlagEnv {
			return "on"
		}
		return ""
	}, edge, zerolog.Nop())
	if got != edge {
		t.Fatalf("flag on returned %T, want the HTTP edge purger", got)
	}
}

func TestUserReceiptPurgerFromEnv_OnWithoutEdgeFailsEveryCall(t *testing.T) {
	t.Parallel()
	got := UserReceiptPurgerFromEnv(func(string) string { return "true" }, nil, zerolog.Nop())
	if got == nil {
		t.Fatal("flag on with no edge hop returned nil; user erasures would be acked and lost")
	}
	if _, err := got.PurgeEdgeUserReceipts("org_a", "user_a"); err == nil {
		t.Fatal("a misconfigured user hop accepted a call")
	}
}

func TestHTTPEdgePurger_UserReceiptsRouteAndEscaping(t *testing.T) {
	t.Parallel()
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodDelete {
			t.Errorf("method=%s want DELETE", r.Method)
		}
		if r.URL.EscapedPath() != "/v1/internal/orgs/org_a/users/user%2Fb/receipts" {
			t.Errorf("path=%s", r.URL.EscapedPath())
		}
		if got := r.Header.Get("Authorization"); got != "Bearer tok" {
			t.Errorf("authorization header=%q want Bearer tok", got)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"data":{"families":[{"family":"step_receipts","rows_deleted":4,"tombstones_deleted":0,"cache_keys_deleted":0,"keys_evicted":false}]}}`))
	}))
	defer srv.Close()

	p := NewHTTPEdgePurger(srv.URL, "tok", nil)
	res, err := p.PurgeEdgeUserReceipts("org_a", "user/b")
	if err != nil {
		t.Fatalf("PurgeEdgeUserReceipts: %v", err)
	}
	if res.Total() != 4 || len(res.Families) != 1 || res.Families[0].Family != "step_receipts" {
		t.Fatalf("result=%+v want one step_receipts family with 4 rows", res)
	}
}

func TestHTTPEdgePurger_UserReceiptsRefusalIsAnError(t *testing.T) {
	t.Parallel()
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNotImplemented)
	}))
	defer srv.Close()
	if _, err := NewHTTPEdgePurger(srv.URL, "tok", nil).PurgeEdgeUserReceipts("org_a", "user_a"); err == nil {
		t.Fatal("a 501 from the edge was accepted")
	}
}
