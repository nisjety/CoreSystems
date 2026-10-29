package sync

import (
	"context"
	"errors"
	"io"
	"testing"

	"github.com/google/uuid"
	"github.com/rs/zerolog"

	"github.com/triodelab/finspo/internal/events"
	"github.com/triodelab/finspo/internal/store"
)

// ---------------------------------------------------------------------------
// fakes
// ---------------------------------------------------------------------------

type fakeProposalExecStore struct {
	prop      store.Proposal
	getErr    error
	setStatus string
	setReason string
	setErr    error
}

func (f *fakeProposalExecStore) Get(_ context.Context, _ uuid.UUID) (store.Proposal, error) {
	return f.prop, f.getErr
}

func (f *fakeProposalExecStore) SetExecutionResult(_ context.Context, _ uuid.UUID, status, reason string) (store.Proposal, error) {
	f.setStatus = status
	f.setReason = reason
	if f.setErr != nil {
		return store.Proposal{}, f.setErr
	}
	out := f.prop
	out.Status = status
	out.FailureReason = reason
	return out, nil
}

type fakeItemExecStore struct {
	targets    map[string]store.ExecTarget // keyed by item PK string
	resolveErr error
	softDeletes []uuid.UUID
}

func (f *fakeItemExecStore) ResolveForExecution(_ context.Context, itemPK uuid.UUID) (store.ExecTarget, error) {
	if f.resolveErr != nil {
		return store.ExecTarget{}, f.resolveErr
	}
	t, ok := f.targets[itemPK.String()]
	if !ok {
		return store.ExecTarget{}, store.ErrNotFound
	}
	return t, nil
}

func (f *fakeItemExecStore) SoftDeleteByPK(_ context.Context, itemPK uuid.UUID) error {
	f.softDeletes = append(f.softDeletes, itemPK)
	return nil
}

type fakeMutator struct {
	deleted   []string
	moved     []string
	deleteErr map[string]error // keyed by itemID
	moveErr   map[string]error
}

func (f *fakeMutator) DeleteItem(_ context.Context, _ string, _ string, itemID string) error {
	if e, ok := f.deleteErr[itemID]; ok {
		return e
	}
	f.deleted = append(f.deleted, itemID)
	return nil
}

func (f *fakeMutator) MoveItem(_ context.Context, _ string, _ string, itemID, dest string) error {
	if e, ok := f.moveErr[itemID]; ok {
		return e
	}
	f.moved = append(f.moved, itemID+"->"+dest)
	return nil
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

func target(pk uuid.UUID, itemID, path string, folder bool) store.ExecTarget {
	return store.ExecTarget{
		ItemPK:         pk,
		OrganizationID: "org-1",
		SourceID:       uuid.New(),
		DriveID:        "drive-1",
		ItemID:         itemID,
		Path:           path,
		IsFolder:       folder,
	}
}

func newExecutor(props *fakeProposalExecStore, items *fakeItemExecStore, mut *fakeMutator, audit *fakeAuditWriter, pub *recordingPublisher, allow bool, archive string) *Executor {
	return NewExecutor(ExecutorConfig{
		Proposals:      props,
		Items:          items,
		Mutator:        mut,
		Audit:          audit,
		Publisher:      pub,
		Subjects:       events.NewSubjects("finspo-test"),
		Logger:         zerolog.New(io.Discard),
		AllowExecution: allow,
		ArchiveFolder:  archive,
	})
}

type fakeAuditWriter struct{ calls []store.AuditInput }

func (f *fakeAuditWriter) Write(_ context.Context, in store.AuditInput) (store.AuditEntry, error) {
	f.calls = append(f.calls, in)
	return store.AuditEntry{}, nil
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

func TestExecuteDisabledReturnsError(t *testing.T) {
	t.Parallel()

	x := newExecutor(&fakeProposalExecStore{}, &fakeItemExecStore{}, &fakeMutator{}, &fakeAuditWriter{}, &recordingPublisher{}, false, "")
	_, err := x.ExecuteProposal(context.Background(), uuid.New(), "alice")
	if !errors.Is(err, ErrExecutionDisabled) {
		t.Fatalf("err = %v, want ErrExecutionDisabled", err)
	}
}

func TestExecuteRejectsUnapprovedProposal(t *testing.T) {
	t.Parallel()

	props := &fakeProposalExecStore{prop: store.Proposal{ID: uuid.New(), Status: store.ProposalStatusPending}}
	x := newExecutor(props, &fakeItemExecStore{}, &fakeMutator{}, &fakeAuditWriter{}, &recordingPublisher{}, true, "")
	_, err := x.ExecuteProposal(context.Background(), props.prop.ID, "alice")
	if !errors.Is(err, ErrNotApproved) {
		t.Fatalf("err = %v, want ErrNotApproved", err)
	}
}

func TestExecuteDeleteHappyPath(t *testing.T) {
	t.Parallel()

	pk1, pk2 := uuid.New(), uuid.New()
	props := &fakeProposalExecStore{prop: store.Proposal{
		ID:             uuid.New(),
		OrganizationID: "org-1",
		Kind:           store.ProposalKindDelete,
		Status:         store.ProposalStatusApproved,
		ItemPKs:        []uuid.UUID{pk1, pk2},
	}}
	items := &fakeItemExecStore{targets: map[string]store.ExecTarget{
		pk1.String(): target(pk1, "item-1", "/a.pdf", false),
		pk2.String(): target(pk2, "item-2", "/b.pdf", false),
	}}
	mut := &fakeMutator{}
	audit := &fakeAuditWriter{}
	pub := &recordingPublisher{}

	x := newExecutor(props, items, mut, audit, pub, true, "")
	res, err := x.ExecuteProposal(context.Background(), props.prop.ID, "alice")
	if err != nil {
		t.Fatalf("ExecuteProposal: %v", err)
	}
	if res.Succeeded != 2 || res.Failed != 0 {
		t.Fatalf("res = %#v", res)
	}
	if len(mut.deleted) != 2 {
		t.Errorf("graph deletes = %d, want 2", len(mut.deleted))
	}
	if len(items.softDeletes) != 2 {
		t.Errorf("local soft-deletes = %d, want 2", len(items.softDeletes))
	}
	if props.setStatus != store.ProposalStatusExecuted {
		t.Errorf("final status = %q, want executed", props.setStatus)
	}
	if got := countSubjects(pub.emitted); got["finspo-test.proposal.executed"] != 1 {
		t.Errorf("executed events = %#v", got)
	}
	// 2 per-item audits.
	if len(audit.calls) != 2 {
		t.Errorf("audit calls = %d, want 2", len(audit.calls))
	}
}

func TestExecutePartialFailureMarksProposalFailed(t *testing.T) {
	t.Parallel()

	pk1, pk2 := uuid.New(), uuid.New()
	props := &fakeProposalExecStore{prop: store.Proposal{
		ID:             uuid.New(),
		OrganizationID: "org-1",
		Kind:           store.ProposalKindDelete,
		Status:         store.ProposalStatusApproved,
		ItemPKs:        []uuid.UUID{pk1, pk2},
	}}
	items := &fakeItemExecStore{targets: map[string]store.ExecTarget{
		pk1.String(): target(pk1, "item-1", "/a.pdf", false),
		pk2.String(): target(pk2, "item-2", "/b.pdf", false),
	}}
	mut := &fakeMutator{deleteErr: map[string]error{"item-2": errors.New("403 accessDenied")}}
	pub := &recordingPublisher{}

	x := newExecutor(props, items, mut, &fakeAuditWriter{}, pub, true, "")
	res, err := x.ExecuteProposal(context.Background(), props.prop.ID, "alice")
	if err != nil {
		t.Fatalf("ExecuteProposal: %v", err)
	}
	if res.Succeeded != 1 || res.Failed != 1 {
		t.Fatalf("res = %#v", res)
	}
	if props.setStatus != store.ProposalStatusFailed {
		t.Errorf("final status = %q, want failed", props.setStatus)
	}
	if got := countSubjects(pub.emitted); got["finspo-test.proposal.failed"] != 1 {
		t.Errorf("failed events = %#v", got)
	}
}

func TestExecuteRefusesFolderTarget(t *testing.T) {
	t.Parallel()

	pk := uuid.New()
	props := &fakeProposalExecStore{prop: store.Proposal{
		ID:             uuid.New(),
		OrganizationID: "org-1",
		Kind:           store.ProposalKindDelete,
		Status:         store.ProposalStatusApproved,
		ItemPKs:        []uuid.UUID{pk},
	}}
	items := &fakeItemExecStore{targets: map[string]store.ExecTarget{
		pk.String(): target(pk, "folder-1", "/Reports", true),
	}}
	mut := &fakeMutator{}

	x := newExecutor(props, items, mut, &fakeAuditWriter{}, &recordingPublisher{}, true, "")
	res, err := x.ExecuteProposal(context.Background(), props.prop.ID, "alice")
	if err != nil {
		t.Fatalf("ExecuteProposal: %v", err)
	}
	if res.Failed != 1 || len(mut.deleted) != 0 {
		t.Fatalf("folder should not be deleted; res=%#v deletes=%v", res, mut.deleted)
	}
}

func TestExecuteArchiveRequiresDestination(t *testing.T) {
	t.Parallel()

	pk := uuid.New()
	props := &fakeProposalExecStore{prop: store.Proposal{
		ID:             uuid.New(),
		OrganizationID: "org-1",
		Kind:           store.ProposalKindArchive,
		Status:         store.ProposalStatusApproved,
		ItemPKs:        []uuid.UUID{pk},
	}}
	x := newExecutor(props, &fakeItemExecStore{}, &fakeMutator{}, &fakeAuditWriter{}, &recordingPublisher{}, true, "")
	_, err := x.ExecuteProposal(context.Background(), props.prop.ID, "alice")
	if !errors.Is(err, ErrArchiveNotConfigured) {
		t.Fatalf("err = %v, want ErrArchiveNotConfigured", err)
	}
	if props.setStatus != store.ProposalStatusFailed {
		t.Errorf("status = %q, want failed", props.setStatus)
	}
}

func TestExecuteArchiveMovesToDestination(t *testing.T) {
	t.Parallel()

	pk := uuid.New()
	props := &fakeProposalExecStore{prop: store.Proposal{
		ID:             uuid.New(),
		OrganizationID: "org-1",
		Kind:           store.ProposalKindArchive,
		Status:         store.ProposalStatusApproved,
		ItemPKs:        []uuid.UUID{pk},
	}}
	items := &fakeItemExecStore{targets: map[string]store.ExecTarget{
		pk.String(): target(pk, "item-1", "/a.pdf", false),
	}}
	mut := &fakeMutator{}
	x := newExecutor(props, items, mut, &fakeAuditWriter{}, &recordingPublisher{}, true, "archive-folder")
	res, err := x.ExecuteProposal(context.Background(), props.prop.ID, "alice")
	if err != nil {
		t.Fatalf("ExecuteProposal: %v", err)
	}
	if res.Succeeded != 1 {
		t.Fatalf("res = %#v", res)
	}
	if len(mut.moved) != 1 || mut.moved[0] != "item-1->archive-folder" {
		t.Errorf("moves = %v", mut.moved)
	}
	if len(items.softDeletes) != 0 {
		t.Errorf("archive must not soft-delete; got %v", items.softDeletes)
	}
}
