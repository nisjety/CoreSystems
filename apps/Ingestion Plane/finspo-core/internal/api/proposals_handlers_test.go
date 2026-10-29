package api

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/google/uuid"

	"github.com/triodelab/finspo/internal/store"
)

// ---------------------------------------------------------------------------
// fakes
// ---------------------------------------------------------------------------

type fakeProposalStore struct {
	createIn      store.CreateProposalInput
	created       store.Proposal
	createErr     error
	got           store.Proposal
	getErr        error
	listOut       []store.Proposal
	listErr       error
	decideIn      decideArgs
	decideOut     store.Proposal
	decideErr     error
}

type decideArgs struct {
	id        uuid.UUID
	newStatus string
	decidedBy string
	notes     string
}

func (f *fakeProposalStore) Create(_ context.Context, in store.CreateProposalInput) (store.Proposal, error) {
	f.createIn = in
	return f.created, f.createErr
}
func (f *fakeProposalStore) Get(_ context.Context, _ uuid.UUID) (store.Proposal, error) {
	return f.got, f.getErr
}
func (f *fakeProposalStore) ListByOrg(_ context.Context, _ string, _ string, _ int) ([]store.Proposal, error) {
	return f.listOut, f.listErr
}
func (f *fakeProposalStore) Decide(_ context.Context, id uuid.UUID, newStatus, decidedBy, notes string) (store.Proposal, error) {
	f.decideIn = decideArgs{id, newStatus, decidedBy, notes}
	return f.decideOut, f.decideErr
}

type fakeAudit struct {
	calls []store.AuditInput
}

func (f *fakeAudit) Write(_ context.Context, in store.AuditInput) (store.AuditEntry, error) {
	f.calls = append(f.calls, in)
	return store.AuditEntry{}, nil
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

func newProposalServer(proposals ProposalStore, audit AuditWriter) *httpReachable {
	app := NewServer(ServerConfig{
		APIKey:    "key",
		Browser:   nil,
		Proposals: proposals,
		Audit:     audit,
	})
	return &httpReachable{app: app}
}

type httpReachable struct{ app interface {
	Test(*http.Request, ...int) (*http.Response, error)
} }

func (h *httpReachable) do(t *testing.T, method, path, body string) *http.Response {
	t.Helper()
	var reader *bytes.Reader
	if body != "" {
		reader = bytes.NewReader([]byte(body))
	} else {
		reader = bytes.NewReader(nil)
	}
	req := httptest.NewRequest(method, path, reader)
	req.Header.Set("X-API-Key", "key")
	req.Header.Set("X-Org-ID", "org-1")
	req.Header.Set("X-User-ID", "alice@example.com")
	req.Header.Set("Content-Type", "application/json")
	resp, err := h.app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	return resp
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

func TestCreateProposalHappyPath(t *testing.T) {
	t.Parallel()

	itemPK := uuid.New()
	created := store.Proposal{
		ID:             uuid.New(),
		OrganizationID: "org-1",
		ProposedBy:     "alice@example.com",
		Kind:           "delete",
		Reason:         "duplicate of newer copy",
		ItemPKs:        []uuid.UUID{itemPK},
		Status:         "pending",
	}
	store := &fakeProposalStore{created: created}
	audit := &fakeAudit{}
	srv := newProposalServer(store, audit)

	body := `{"kind":"delete","reason":"duplicate of newer copy","item_pks":["` + itemPK.String() + `"]}`
	resp := srv.do(t, http.MethodPost, "/api/v1/proposals", body)
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusCreated {
		t.Fatalf("status = %d", resp.StatusCode)
	}
	if store.createIn.ProposedBy != "alice@example.com" {
		t.Errorf("ProposedBy = %q", store.createIn.ProposedBy)
	}
	if store.createIn.Kind != "delete" {
		t.Errorf("Kind = %q", store.createIn.Kind)
	}
	if len(store.createIn.ItemPKs) != 1 || store.createIn.ItemPKs[0] != itemPK {
		t.Errorf("ItemPKs = %#v", store.createIn.ItemPKs)
	}
	if len(audit.calls) != 1 || audit.calls[0].Action != "proposal.created" {
		t.Errorf("audit = %#v", audit.calls)
	}
}

func TestCreateProposalRejectsBadKind(t *testing.T) {
	t.Parallel()

	srv := newProposalServer(&fakeProposalStore{}, &fakeAudit{})
	resp := srv.do(t, http.MethodPost, "/api/v1/proposals",
		`{"kind":"nuke","reason":"r","item_pks":["00000000-0000-0000-0000-000000000001"]}`)
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400", resp.StatusCode)
	}
}

func TestCreateProposalRejectsEmptyItems(t *testing.T) {
	t.Parallel()

	srv := newProposalServer(&fakeProposalStore{}, &fakeAudit{})
	resp := srv.do(t, http.MethodPost, "/api/v1/proposals",
		`{"kind":"delete","reason":"r","item_pks":[]}`)
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400", resp.StatusCode)
	}
}

func TestApproveProposalReturns404WhenWrongOrg(t *testing.T) {
	t.Parallel()

	other := store.Proposal{ID: uuid.New(), OrganizationID: "other-org", Status: "pending"}
	store := &fakeProposalStore{got: other}
	srv := newProposalServer(store, &fakeAudit{})

	resp := srv.do(t, http.MethodPost, "/api/v1/proposals/"+other.ID.String()+"/approve", `{}`)
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusNotFound {
		t.Fatalf("status = %d, want 404 (tenant guard)", resp.StatusCode)
	}
}

func TestApproveProposalReturns409OnInvalidTransition(t *testing.T) {
	t.Parallel()

	id := uuid.New()
	store := &fakeProposalStore{
		got:       store.Proposal{ID: id, OrganizationID: "org-1", Status: "approved"},
		decideErr: store.ErrInvalidTransition,
	}
	srv := newProposalServer(store, &fakeAudit{})

	resp := srv.do(t, http.MethodPost, "/api/v1/proposals/"+id.String()+"/approve", `{}`)
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusConflict {
		t.Fatalf("status = %d, want 409", resp.StatusCode)
	}
}

func TestApproveProposalWritesAuditOnSuccess(t *testing.T) {
	t.Parallel()

	id := uuid.New()
	store := &fakeProposalStore{
		got:       store.Proposal{ID: id, OrganizationID: "org-1", Status: "pending"},
		decideOut: store.Proposal{ID: id, OrganizationID: "org-1", Kind: "delete", Status: "approved"},
	}
	audit := &fakeAudit{}
	srv := newProposalServer(store, audit)

	resp := srv.do(t, http.MethodPost, "/api/v1/proposals/"+id.String()+"/approve", `{"notes":"ok"}`)
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d", resp.StatusCode)
	}
	if store.decideIn.newStatus != "approved" {
		t.Errorf("decided status = %q", store.decideIn.newStatus)
	}
	if len(audit.calls) != 1 || audit.calls[0].Action != "proposal.approved" {
		t.Errorf("audit = %#v", audit.calls)
	}
}

func TestListProposalsRoundTrip(t *testing.T) {
	t.Parallel()

	listed := []store.Proposal{
		{ID: uuid.New(), OrganizationID: "org-1", Kind: "delete", Status: "pending"},
		{ID: uuid.New(), OrganizationID: "org-1", Kind: "archive", Status: "approved"},
	}
	srv := newProposalServer(&fakeProposalStore{listOut: listed}, &fakeAudit{})

	resp := srv.do(t, http.MethodGet, "/api/v1/proposals", "")
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d", resp.StatusCode)
	}
	var payload struct {
		Data struct {
			Count int `json:"count"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&payload); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if payload.Data.Count != 2 {
		t.Errorf("count = %d, want 2", payload.Data.Count)
	}
}
