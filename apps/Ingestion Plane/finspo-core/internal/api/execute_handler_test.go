package api

import (
	"context"
	"net/http"
	"testing"

	"github.com/google/uuid"

	"github.com/triodelab/finspo/internal/store"
	"github.com/triodelab/finspo/internal/sync"
)

type fakeExecutor struct {
	enabled  bool
	res      sync.ExecResult
	err      error
	calledID uuid.UUID
}

func (f *fakeExecutor) Enabled() bool { return f.enabled }

func (f *fakeExecutor) ExecuteProposal(_ context.Context, id uuid.UUID, _ string) (sync.ExecResult, error) {
	f.calledID = id
	return f.res, f.err
}

func newExecuteServer(proposals ProposalStore, executor ProposalExecutor) *httpReachable {
	app := NewServer(ServerConfig{
		APIKey:    "key",
		Proposals: proposals,
		Executor:  executor,
	})
	return &httpReachable{app: app}
}

func TestExecuteEndpointDisabledReturns503(t *testing.T) {
	t.Parallel()

	id := uuid.New()
	props := &fakeProposalStore{got: store.Proposal{ID: id, OrganizationID: "org-1", Status: "approved"}}
	srv := newExecuteServer(props, &fakeExecutor{enabled: false})

	resp := srv.do(t, http.MethodPost, "/api/v1/proposals/"+id.String()+"/execute", `{}`)
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want 503", resp.StatusCode)
	}
}

func TestExecuteEndpointWrongOrgReturns404(t *testing.T) {
	t.Parallel()

	id := uuid.New()
	props := &fakeProposalStore{got: store.Proposal{ID: id, OrganizationID: "other-org", Status: "approved"}}
	srv := newExecuteServer(props, &fakeExecutor{enabled: true})

	resp := srv.do(t, http.MethodPost, "/api/v1/proposals/"+id.String()+"/execute", `{}`)
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusNotFound {
		t.Fatalf("status = %d, want 404 (tenant guard before execution)", resp.StatusCode)
	}
}

func TestExecuteEndpointNotApprovedReturns409(t *testing.T) {
	t.Parallel()

	id := uuid.New()
	props := &fakeProposalStore{got: store.Proposal{ID: id, OrganizationID: "org-1", Status: "approved"}}
	exec := &fakeExecutor{enabled: true, err: sync.ErrNotApproved}
	srv := newExecuteServer(props, exec)

	resp := srv.do(t, http.MethodPost, "/api/v1/proposals/"+id.String()+"/execute", `{}`)
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusConflict {
		t.Fatalf("status = %d, want 409", resp.StatusCode)
	}
}

func TestExecuteEndpointHappyPath(t *testing.T) {
	t.Parallel()

	id := uuid.New()
	props := &fakeProposalStore{got: store.Proposal{ID: id, OrganizationID: "org-1", Status: "approved"}}
	exec := &fakeExecutor{enabled: true, res: sync.ExecResult{ProposalID: id.String(), Status: "executed", Succeeded: 3}}
	srv := newExecuteServer(props, exec)

	resp := srv.do(t, http.MethodPost, "/api/v1/proposals/"+id.String()+"/execute", `{}`)
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want 200", resp.StatusCode)
	}
	if exec.calledID != id {
		t.Errorf("executor called with %s, want %s", exec.calledID, id)
	}
}

func TestExecuteEndpointArchiveNotConfiguredReturns400(t *testing.T) {
	t.Parallel()

	id := uuid.New()
	props := &fakeProposalStore{got: store.Proposal{ID: id, OrganizationID: "org-1", Status: "approved"}}
	exec := &fakeExecutor{enabled: true, err: sync.ErrArchiveNotConfigured}
	srv := newExecuteServer(props, exec)

	resp := srv.do(t, http.MethodPost, "/api/v1/proposals/"+id.String()+"/execute", `{}`)
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400", resp.StatusCode)
	}
}
