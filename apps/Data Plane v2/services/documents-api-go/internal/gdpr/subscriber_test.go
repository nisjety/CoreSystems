package gdpr

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
)

type fakeTransferrer struct {
	calls [][3]string // org, from, to
	ret   int64
	err   error
}

func (f *fakeTransferrer) TransferOwnership(_ context.Context, orgID, from, to string) (int64, error) {
	f.calls = append(f.calls, [3]string{orgID, from, to})
	return f.ret, f.err
}

type fakePublisher struct {
	subject string
	data    []byte
	calls   int
}

func (p *fakePublisher) Publish(subject string, data []byte) error {
	p.subject = subject
	p.data = data
	p.calls++
	return nil
}

func TestHandleErasureTransfersUserDocsAndEmits(t *testing.T) {
	repo := &fakeTransferrer{ret: 3}
	pub := &fakePublisher{}
	payload := []byte(`{"subject_type":"user","subject_id":"user-a","org_id":"org-1","requested_by":"admin","ts":"now"}`)

	n, err := HandleErasure(context.Background(), repo, pub, payload)
	if err != nil {
		t.Fatalf("HandleErasure: %v", err)
	}
	if n != 3 {
		t.Fatalf("transferred = %d, want 3", n)
	}
	// Transferred the erased user's docs to the system account, org-scoped.
	if len(repo.calls) != 1 || repo.calls[0] != [3]string{"org-1", "user-a", systemAccount} {
		t.Fatalf("TransferOwnership calls = %v", repo.calls)
	}
	// Emitted ownership.transferred with the count.
	if pub.calls != 1 || pub.subject != OwnershipTransferredSubject {
		t.Fatalf("publish = (%d, %q), want (1, %q)", pub.calls, pub.subject, OwnershipTransferredSubject)
	}
	var emitted map[string]any
	if err := json.Unmarshal(pub.data, &emitted); err != nil {
		t.Fatalf("emitted decode: %v", err)
	}
	if emitted["from_user"] != "user-a" || emitted["to_owner"] != systemAccount {
		t.Fatalf("emitted payload = %v", emitted)
	}
	if emitted["documents_transferred"].(float64) != 3 {
		t.Fatalf("emitted documents_transferred = %v, want 3", emitted["documents_transferred"])
	}
}

func TestHandleErasureIgnoresNonUserAndMissingScope(t *testing.T) {
	for _, payload := range []string{
		`{"subject_type":"team","subject_id":"team-1","org_id":"org-1"}`,
		`{"subject_type":"user","subject_id":"","org_id":"org-1"}`,
		`{"subject_type":"user","subject_id":"u","org_id":""}`,
	} {
		repo := &fakeTransferrer{ret: 9}
		pub := &fakePublisher{}
		n, err := HandleErasure(context.Background(), repo, pub, []byte(payload))
		if err != nil || n != 0 {
			t.Fatalf("payload %s: (%d, %v), want (0, nil)", payload, n, err)
		}
		if len(repo.calls) != 0 || pub.calls != 0 {
			t.Fatalf("payload %s should be a no-op; transfer=%d publish=%d", payload, len(repo.calls), pub.calls)
		}
	}
}

func TestHandleErasurePropagatesTransferError(t *testing.T) {
	repo := &fakeTransferrer{err: errors.New("db down")}
	pub := &fakePublisher{}
	payload := []byte(`{"subject_type":"user","subject_id":"user-a","org_id":"org-1"}`)
	if _, err := HandleErasure(context.Background(), repo, pub, payload); err == nil {
		t.Fatal("expected transfer error to propagate")
	}
	// Must NOT emit ownership.transferred when the transfer failed.
	if pub.calls != 0 {
		t.Fatalf("must not publish on transfer failure; publish=%d", pub.calls)
	}
}
