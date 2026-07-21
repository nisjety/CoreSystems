package gdpr

import (
	"context"
	"errors"
	"testing"
)

type fakeOrgPurger struct {
	calls []string
	err   error
}

func (f *fakeOrgPurger) HardPurgeByOrg(_ context.Context, orgID string) error {
	f.calls = append(f.calls, orgID)
	return f.err
}

func TestHandleOrgErasurePurgesTheOrgFromPayload(t *testing.T) {
	repo := &fakeOrgPurger{}
	payload := []byte(`{"subject_type":"organization","subject_id":"org-1","org_id":"org-1","requested_by":"admin","ts":"2026-07-20T00:00:00Z"}`)

	if err := HandleOrgErasure(context.Background(), repo, payload); err != nil {
		t.Fatalf("HandleOrgErasure: %v", err)
	}
	if len(repo.calls) != 1 || repo.calls[0] != "org-1" {
		t.Fatalf("HardPurgeByOrg calls = %v, want [org-1]", repo.calls)
	}
}

// A per-user erasure fan-out (subscriber.go's concern) arrives on the exact
// same subject. HandleOrgErasure must skip it silently — no purge call, no
// error, no DLQ-worthy poison — since it is a well-formed event this
// consumer simply doesn't own.
func TestHandleOrgErasureSkipsNonOrganizationSubjects(t *testing.T) {
	for _, payload := range []string{
		`{"event_id":"gdpr:fanout:child-1","operation_id":"gdpr:operation-1","subject_type":"user","subject_id":"user-a","org_id":"org-1"}`,
		`{"subject_type":"user_anonymize","subject_id":"user-a","org_id":"org-1"}`,
		`{"subject_type":"team","subject_id":"team-1","org_id":"org-1"}`,
	} {
		repo := &fakeOrgPurger{}
		if err := HandleOrgErasure(context.Background(), repo, []byte(payload)); err != nil {
			t.Fatalf("payload %s: unexpected error %v", payload, err)
		}
		if len(repo.calls) != 0 {
			t.Fatalf("payload %s: must not purge, calls=%v", payload, repo.calls)
		}
	}
}

func TestHandleOrgErasureRejectsMalformedOrganizationEvents(t *testing.T) {
	for _, payload := range []string{
		`{"subject_type":"organization","subject_id":"org-1","org_id":""}`,
		`{"subject_type":"organization","subject_id":"","org_id":"org-1"}`,
		`{"subject_type":"organization"`, // malformed JSON
	} {
		repo := &fakeOrgPurger{}
		err := HandleOrgErasure(context.Background(), repo, []byte(payload))
		if err == nil {
			t.Fatalf("payload %s: expected poison error", payload)
		}
		var poison *poisonOrgEventError
		if !errors.As(err, &poison) {
			t.Fatalf("payload %s: error %v is not classified as poison (would retry forever)", payload, err)
		}
		if len(repo.calls) != 0 {
			t.Fatalf("payload %s: must not purge on malformed input, calls=%v", payload, repo.calls)
		}
	}
}

func TestHandleOrgErasurePropagatesPurgeError(t *testing.T) {
	repo := &fakeOrgPurger{err: errors.New("postgres unavailable")}
	payload := []byte(`{"subject_type":"organization","subject_id":"org-1","org_id":"org-1"}`)

	err := HandleOrgErasure(context.Background(), repo, payload)
	if err == nil {
		t.Fatal("expected purge error to propagate")
	}
	var poison *poisonOrgEventError
	if errors.As(err, &poison) {
		t.Fatalf("a transient DB failure must not be classified as poison: %v", err)
	}
}

// The safety contract: HandleOrgErasure must scope by the payload's org_id
// field, never subject_id, even if a future producer lets them diverge.
func TestHandleOrgErasureScopesByOrgIDFieldNotSubjectID(t *testing.T) {
	repo := &fakeOrgPurger{}
	payload := []byte(`{"subject_type":"organization","subject_id":"org-legacy-alias","org_id":"org-real","requested_by":"admin"}`)

	if err := HandleOrgErasure(context.Background(), repo, payload); err != nil {
		t.Fatalf("HandleOrgErasure: %v", err)
	}
	if len(repo.calls) != 1 || repo.calls[0] != "org-real" {
		t.Fatalf("purge scoped to %v, want [org-real]", repo.calls)
	}
}
