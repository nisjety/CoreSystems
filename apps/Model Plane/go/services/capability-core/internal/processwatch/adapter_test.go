package processwatch

import (
	"context"
	"errors"
	"strings"
	"testing"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/watch"
)

type fakeRegistry struct {
	process   *mpv1.Process
	getErr    error
	page      *mpv1.ReadProcessOutputResponse
	readErr   error
	readCalls int
	lastRead  *mpv1.ReadProcessOutputRequest
	lastAuth  string
}

func (f *fakeRegistry) GetProcess(ctx context.Context, in *mpv1.GetProcessRequest, _ ...grpc.CallOption) (*mpv1.GetProcessResponse, error) {
	if md, ok := metadata.FromOutgoingContext(ctx); ok {
		if values := md.Get("authorization"); len(values) > 0 {
			f.lastAuth = values[0]
		}
	}
	if f.getErr != nil {
		return nil, f.getErr
	}
	return &mpv1.GetProcessResponse{Process: f.process}, nil
}

func (f *fakeRegistry) ReadProcessOutput(_ context.Context, in *mpv1.ReadProcessOutputRequest, _ ...grpc.CallOption) (*mpv1.ReadProcessOutputResponse, error) {
	f.readCalls++
	f.lastRead = in
	if f.readErr != nil {
		return nil, f.readErr
	}
	if f.page != nil {
		return f.page, nil
	}
	return &mpv1.ReadProcessOutputResponse{}, nil
}

type fakeTokens struct {
	token string
	err   error
}

func (f fakeTokens) Token(context.Context, string) (string, error) {
	if f.err != nil {
		return "", f.err
	}
	if f.token == "" {
		return "token-1", nil
	}
	return f.token, nil
}

func liveProcess() *mpv1.Process {
	return &mpv1.Process{
		ProcessId: "proc-1",
		OrgId:     "org-1",
		SpaceId:   "space-1",
		State:     mpv1.ProcessState_PROCESS_STATE_RUNNING,
	}
}

func watchFor(process string) watch.Watch {
	return watch.Watch{
		ID: "wch_1", OrgID: "org-1", SpaceRef: "space-1",
		SourceKind: watch.SourceKindProcessOutput, SourceRef: process,
		Predicate: watch.Predicate{Kind: watch.PredicateAny},
		State:     watch.StateActive, TriggerMode: watch.TriggerOnce,
	}
}

func newTestAdapter(t *testing.T, registry *fakeRegistry) *Adapter {
	t.Helper()
	adapter, err := NewAdapter(registry, fakeTokens{})
	if err != nil {
		t.Fatalf("NewAdapter: %v", err)
	}
	return adapter
}

// THE security property of this adapter. capability-core's sandbox-manager
// credential is org-wide, so the registry will resolve any process id in the
// organization for it — and `source_ref` is caller-supplied. This comparison is
// the only thing between a watch naming an arbitrary id and another Space's
// output.
func TestPollRefusesAProcessInAnotherSpace(t *testing.T) {
	t.Parallel()
	elsewhere := liveProcess()
	elsewhere.SpaceId = "space-2"
	registry := &fakeRegistry{process: elsewhere}

	_, err := newTestAdapter(t, registry).Poll(context.Background(), watchFor("proc-1"))
	if !errors.Is(err, ErrSourceGone) {
		t.Fatalf("err = %v, want ErrSourceGone", err)
	}
	if registry.readCalls != 0 {
		t.Fatal("output was read for a process in another Space")
	}
}

// The refusals are indistinguishable on purpose: telling a caller that an id
// exists somewhere else makes the watch an oracle for which ids exist.
func TestAMissingProcessAndAForeignOneAreTheSameAnswer(t *testing.T) {
	t.Parallel()
	missing := &fakeRegistry{getErr: status.Error(codes.NotFound, "no such process")}
	_, missingErr := newTestAdapter(t, missing).Poll(context.Background(), watchFor("proc-1"))

	foreign := liveProcess()
	foreign.SpaceId = "space-2"
	_, foreignErr := newTestAdapter(t, &fakeRegistry{process: foreign}).Poll(context.Background(), watchFor("proc-1"))

	if !errors.Is(missingErr, ErrSourceGone) || !errors.Is(foreignErr, ErrSourceGone) {
		t.Fatalf("errors differ: missing=%v foreign=%v", missingErr, foreignErr)
	}
	if missingErr.Error() != foreignErr.Error() {
		t.Fatalf("the two refusals are distinguishable:\n  %q\n  %q", missingErr, foreignErr)
	}
}

// A watch created under audience revision N must not surface content recorded
// under a later audience — the same ceiling Session Core applies to runs.
func TestPollRefusesContentAboveTheWatchesAudienceCeiling(t *testing.T) {
	t.Parallel()
	later := liveProcess()
	later.RecipientAudienceRevision = 9
	registry := &fakeRegistry{process: later}

	w := watchFor("proc-1")
	w.Authority.RecipientAudienceRevision = 4
	if _, err := newTestAdapter(t, registry).Poll(context.Background(), w); !errors.Is(err, ErrSourceGone) {
		t.Fatalf("err = %v, want ErrSourceGone for a process above the ceiling", err)
	}
	if registry.readCalls != 0 {
		t.Fatal("output was read above the watch's audience ceiling")
	}

	// At or below the ceiling it reads normally.
	within := liveProcess()
	within.RecipientAudienceRevision = 4
	ok := &fakeRegistry{process: within}
	if _, err := newTestAdapter(t, ok).Poll(context.Background(), w); err != nil {
		t.Fatalf("a process at the ceiling was refused: %v", err)
	}
}

// A process recorded before the ceiling existed reads as visible — the same
// meaning Session Core gives a NULL revision on a thread.
func TestAZeroRevisionProcessIsVisible(t *testing.T) {
	t.Parallel()
	registry := &fakeRegistry{process: liveProcess()} // revision 0
	w := watchFor("proc-1")
	w.Authority.RecipientAudienceRevision = 4
	if _, err := newTestAdapter(t, registry).Poll(context.Background(), w); err != nil {
		t.Fatalf("a pre-ceiling process was refused: %v", err)
	}
}

func TestPollCarriesTheCredentialAndTheCursor(t *testing.T) {
	t.Parallel()
	registry := &fakeRegistry{process: liveProcess()}
	w := watchFor("proc-1")
	w.CursorValue = 12

	if _, err := newTestAdapter(t, registry).Poll(context.Background(), w); err != nil {
		t.Fatalf("Poll: %v", err)
	}
	if registry.lastAuth != "Bearer token-1" {
		t.Fatalf("authorization = %q", registry.lastAuth)
	}
	if registry.lastRead.GetAfterSeq() != 12 {
		t.Fatalf("read after_seq = %d, want the watch's cursor", registry.lastRead.GetAfterSeq())
	}
	if registry.lastRead.GetMaxBytes() != MaxReadBytes {
		t.Fatalf("read budget = %d, want %d", registry.lastRead.GetMaxBytes(), MaxReadBytes)
	}
	if registry.lastRead.GetSpaceId() != "space-1" {
		t.Fatalf("the read did not name the Space: %q", registry.lastRead.GetSpaceId())
	}
}

// A terminal process is only DRAINED once a read comes back empty. Reporting
// terminal while output remains ends the watch before its last lines — exactly
// where a failing build says why it failed.
func TestATerminalProcessIsNotDrainedWhileOutputRemains(t *testing.T) {
	t.Parallel()
	exited := liveProcess()
	exited.State = mpv1.ProcessState_PROCESS_STATE_EXITED
	code := int32(2)
	exited.ExitCode = &code

	registry := &fakeRegistry{
		process: exited,
		page: &mpv1.ReadProcessOutputResponse{
			Chunks: []*mpv1.ProcessOutputChunk{out(5, "ERROR: could not link\n")},
		},
	}
	result, err := newTestAdapter(t, registry).Poll(context.Background(), watchFor("proc-1"))
	if err != nil {
		t.Fatalf("Poll: %v", err)
	}
	if result.SourceTerminal {
		t.Fatal("the source was reported terminal while a page of output was still arriving")
	}
	if len(result.Lines) != 1 {
		t.Fatalf("got %d lines, want the final one", len(result.Lines))
	}
}

func TestADrainedTerminalProcessReportsItsOutcome(t *testing.T) {
	t.Parallel()
	exited := liveProcess()
	exited.State = mpv1.ProcessState_PROCESS_STATE_EXITED
	code := int32(2)
	exited.ExitCode = &code

	registry := &fakeRegistry{
		process: exited,
		page:    &mpv1.ReadProcessOutputResponse{NextCursor: 9},
	}
	result, err := newTestAdapter(t, registry).Poll(context.Background(), watchFor("proc-1"))
	if err != nil {
		t.Fatalf("Poll: %v", err)
	}
	if !result.SourceTerminal {
		t.Fatal("a drained terminal process was not reported terminal")
	}
	if !strings.Contains(result.TerminalSummary, "code 2") {
		t.Fatalf("terminal summary = %q, want the exit code", result.TerminalSummary)
	}
	if result.Cursor != 9 {
		t.Fatalf("cursor = %d, want the registry's own next cursor once drained", result.Cursor)
	}
}

// LOST is the honest one: the host went away and nobody can say what happened.
func TestALostProcessSaysItsOutcomeIsUnknown(t *testing.T) {
	t.Parallel()
	lost := liveProcess()
	lost.State = mpv1.ProcessState_PROCESS_STATE_LOST
	registry := &fakeRegistry{process: lost}

	result, err := newTestAdapter(t, registry).Poll(context.Background(), watchFor("proc-1"))
	if err != nil {
		t.Fatalf("Poll: %v", err)
	}
	if !strings.Contains(result.TerminalSummary, "unknown") {
		t.Fatalf("terminal summary = %q; a lost host must not read as a clean finish", result.TerminalSummary)
	}
}

// Never swallowed: a watch that skipped output without saying so is worse than
// one that says it did.
func TestAGapIsForwarded(t *testing.T) {
	t.Parallel()
	registry := &fakeRegistry{
		process: liveProcess(),
		page:    &mpv1.ReadProcessOutputResponse{GapBefore: true, Chunks: []*mpv1.ProcessOutputChunk{out(40, "late\n")}},
	}
	result, err := newTestAdapter(t, registry).Poll(context.Background(), watchFor("proc-1"))
	if err != nil {
		t.Fatalf("Poll: %v", err)
	}
	if !result.GapBefore {
		t.Fatal("the registry reported a retention gap and the adapter dropped it")
	}
}

// A token failure is a transient condition, not a reason to end a watch: it
// must surface as an error the sweeper backs off on, never as ErrSourceGone.
func TestATokenFailureIsNotASourceGone(t *testing.T) {
	t.Parallel()
	adapter, err := NewAdapter(&fakeRegistry{process: liveProcess()}, fakeTokens{err: errors.New("auth core down")})
	if err != nil {
		t.Fatalf("NewAdapter: %v", err)
	}
	_, pollErr := adapter.Poll(context.Background(), watchFor("proc-1"))
	if pollErr == nil {
		t.Fatal("a token failure was not reported")
	}
	if errors.Is(pollErr, ErrSourceGone) {
		t.Fatal("a transient credential failure was reported as the source being gone; that would cancel a person's watch over an outage")
	}
}

// Same reasoning for a read that fails for any reason other than not-found.
func TestATransientReadFailureIsNotASourceGone(t *testing.T) {
	t.Parallel()
	registry := &fakeRegistry{
		process: liveProcess(),
		readErr: status.Error(codes.Unavailable, "sandbox-manager is restarting"),
	}
	_, err := newTestAdapter(t, registry).Poll(context.Background(), watchFor("proc-1"))
	if err == nil {
		t.Fatal("a failed read was not reported")
	}
	if errors.Is(err, ErrSourceGone) {
		t.Fatal("an unavailable registry was reported as the source being gone")
	}
}

func TestKindMatchesTheSourceVocabulary(t *testing.T) {
	t.Parallel()
	if got := newTestAdapter(t, &fakeRegistry{}).Kind(); got != watch.SourceKindProcessOutput {
		t.Fatalf("Kind() = %q, want %q", got, watch.SourceKindProcessOutput)
	}
	// And the adapter set accepts it, which is what the sweeper resolves through.
	if _, err := watch.NewAdapterSet(newTestAdapter(t, &fakeRegistry{})); err != nil {
		t.Fatalf("the adapter was refused by NewAdapterSet: %v", err)
	}
}
