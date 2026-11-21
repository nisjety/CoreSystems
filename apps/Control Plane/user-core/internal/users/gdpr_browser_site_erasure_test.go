package users

import (
	"context"
	"errors"
	"reflect"
	"strings"
	"testing"
	"time"
)

// fanoutOnlyStore serves the fan-out half of the erasure store. The operation under test is
// stamped past the auth, local and audit stages, so processClaimedErasure reaches only these
// methods; any other call panics on the nil embedded interface, which a test should see.
type fanoutOnlyStore struct {
	erasureOperationStore
	pending      []ErasureFanoutChild
	completed    []string
	failed       []string
	failMessages []string
	failTerminal []bool
	fanoutDone   bool
	events       *[]string
}

func (f *fanoutOnlyStore) ClaimNextErasureFanout(context.Context, string) (ErasureFanoutChild, bool, error) {
	if len(f.pending) == 0 {
		return ErasureFanoutChild{}, false, nil
	}
	child := f.pending[0]
	f.pending = f.pending[1:]
	return child, true, nil
}

func (f *fanoutOnlyStore) FailErasureFanout(_ context.Context, childEventID string, _ int, _ time.Time, message string, terminal bool) error {
	f.failed = append(f.failed, childEventID)
	f.failMessages = append(f.failMessages, message)
	f.failTerminal = append(f.failTerminal, terminal)
	return nil
}

func (f *fanoutOnlyStore) CompleteErasureFanoutChild(_ context.Context, childEventID string, _ int) error {
	f.completed = append(f.completed, childEventID)
	return nil
}

func (f *fanoutOnlyStore) CompleteErasureFanout(context.Context, string, int) error {
	f.fanoutDone = true
	return nil
}

func (f *fanoutOnlyStore) GetErasureOperation(_ context.Context, operationID string) (ErasureOperation, error) {
	return ErasureOperation{OperationID: operationID}, nil
}

// orderedFanoutPublisher records each publish into the shared event log.
type orderedFanoutPublisher struct {
	events   *[]string
	eventIDs []string
}

func (p *orderedFanoutPublisher) PublishGDPRErasure(_ context.Context, eventID string, _ []byte) error {
	p.eventIDs = append(p.eventIDs, eventID)
	*p.events = append(*p.events, "publish:"+eventID)
	return nil
}

type browserSiteEraseCall struct {
	orgID  string
	userID string
}

// recordingBrowserSiteEraser records each org-core erase into the shared event log and answers
// the error configured for an organisation in failOrgs.
type recordingBrowserSiteEraser struct {
	events   *[]string
	calls    []browserSiteEraseCall
	failOrgs map[string]error
}

func (e *recordingBrowserSiteEraser) EraseBrowserSiteChoices(_ context.Context, orgID, userID string) error {
	e.calls = append(e.calls, browserSiteEraseCall{orgID: orgID, userID: userID})
	*e.events = append(*e.events, "erase:"+orgID)
	return e.failOrgs[orgID]
}

// acceptingBrowserSiteEraser answers every org-core erase as done. The saga tests whose subject
// is not the browser-site step wire it, so their fan-out reaches its own publish path.
type acceptingBrowserSiteEraser struct{}

func (acceptingBrowserSiteEraser) EraseBrowserSiteChoices(context.Context, string, string) error {
	return nil
}

// browserSiteFanout builds a service whose fan-out holds the given children, with a recording
// org-core eraser wired (F-f02-16). The eraser is returned so a test can set its failures.
func browserSiteFanout(children ...ErasureFanoutChild) (*Service, *fanoutOnlyStore, *recordingBrowserSiteEraser, *[]string) {
	events := []string{}
	store := &fanoutOnlyStore{pending: children, events: &events}
	publisher := &orderedFanoutPublisher{events: &events}
	eraser := &recordingBrowserSiteEraser{events: &events}
	svc := &Service{erasureStore: store, erasureFanout: publisher}
	svc.SetBrowserSiteEraser(eraser)
	return svc, store, eraser, &events
}

// browserSiteOperation is an erasure of user-1 requested by admin-9, past its earlier stages.
func browserSiteOperation() ErasureOperation {
	now := time.Now().UTC()
	return ErasureOperation{
		OperationID: "gdpr:user-core:hard_delete:fixture", UserID: "user-1", Mode: ErasureModeHardDelete,
		ActorID: "admin-9", ActorRole: "admin", OrgID: "org-1", Attempts: 1, CreatedAt: now,
		AuthCompletedAt: &now, LocalCompletedAt: &now, AuditEnqueuedAt: &now,
	}
}

func browserSiteChild(orgID string, attempts int) ErasureFanoutChild {
	return ErasureFanoutChild{
		ChildEventID: "gdpr:fanout:" + orgID, OperationID: "gdpr:user-core:hard_delete:fixture",
		OrgID: orgID, Attempts: attempts,
	}
}

// TestBrowserSiteErasureRunsBeforeEachChildIsPublished pins the order: each child's organisation
// is erased for the erased user, never the actor, before its event is published. There is no
// switch, so the erase runs for every fan-out.
func TestBrowserSiteErasureRunsBeforeEachChildIsPublished(t *testing.T) {
	svc, store, eraser, events := browserSiteFanout(browserSiteChild("org-1", 1), browserSiteChild("org-2", 1))

	if _, err := svc.processClaimedErasure(context.Background(), browserSiteOperation()); err != nil {
		t.Fatalf("fan-out with the eraser wired: %v", err)
	}
	wantCalls := []browserSiteEraseCall{{orgID: "org-1", userID: "user-1"}, {orgID: "org-2", userID: "user-1"}}
	if !reflect.DeepEqual(eraser.calls, wantCalls) {
		t.Fatalf("org-core calls = %v, want %v", eraser.calls, wantCalls)
	}
	wantOrder := []string{"erase:org-1", "publish:gdpr:fanout:org-1", "erase:org-2", "publish:gdpr:fanout:org-2"}
	if !reflect.DeepEqual(*events, wantOrder) {
		t.Fatalf("events = %v, want %v", *events, wantOrder)
	}
	if !store.fanoutDone || len(store.failed) != 0 || len(store.completed) != 2 {
		t.Fatalf("fan-out must complete both children: done=%v failed=%v completed=%v", store.fanoutDone, store.failed, store.completed)
	}
}

// TestBrowserSiteErasureFailureHoldsTheChildAndItsPublish pins that a failed org-core erase is
// recorded as a retry of that child, its event is not published, the earlier child stays published,
// and the fan-out does not complete.
func TestBrowserSiteErasureFailureHoldsTheChildAndItsPublish(t *testing.T) {
	svc, store, eraser, events := browserSiteFanout(browserSiteChild("org-1", 1), browserSiteChild("org-2", 1))
	eraser.failOrgs = map[string]error{"org-2": errors.New("org-core browser-site erasure endpoint returned status 503")}

	_, err := svc.processClaimedErasure(context.Background(), browserSiteOperation())
	if err == nil || !strings.Contains(err.Error(), "erase browser site choices") {
		t.Fatalf("a failed erase must fail the fan-out, got %v", err)
	}
	if wantOrder := []string{"erase:org-1", "publish:gdpr:fanout:org-1", "erase:org-2"}; !reflect.DeepEqual(*events, wantOrder) {
		t.Fatalf("events = %v, want %v", *events, wantOrder)
	}
	if !reflect.DeepEqual(store.failed, []string{"gdpr:fanout:org-2"}) || store.failTerminal[0] {
		t.Fatalf("org-2 must be held for retry, not terminal: failed=%v terminal=%v", store.failed, store.failTerminal)
	}
	if !reflect.DeepEqual(store.completed, []string{"gdpr:fanout:org-1"}) || store.fanoutDone {
		t.Fatalf("only org-1 may complete and the fan-out must stay open: completed=%v done=%v", store.completed, store.fanoutDone)
	}
}

// TestBrowserSiteErasureWithoutAnEraserFailsClosed pins that with no eraser wired the fan-out
// publishes nothing and completes nothing, so the erasure stays open until org-core is configured.
func TestBrowserSiteErasureWithoutAnEraserFailsClosed(t *testing.T) {
	svc, store, _, events := browserSiteFanout(browserSiteChild("org-1", 1))
	svc.SetBrowserSiteEraser(nil)

	if _, err := svc.processClaimedErasure(context.Background(), browserSiteOperation()); err == nil {
		t.Fatal("a fan-out with no eraser must not complete")
	}
	if len(*events) != 0 || store.fanoutDone {
		t.Fatalf("nothing may be published or completed: events=%v done=%v", *events, store.fanoutDone)
	}
	if len(store.failMessages) != 1 || !strings.Contains(store.failMessages[0], "no org-core eraser configured") {
		t.Fatalf("the child must be held with the refusal, got %v", store.failMessages)
	}
}

// TestBrowserSiteErasureFailureBecomesTerminalAtTheAttemptCap pins that a child whose erase keeps
// failing is marked terminal at the same attempt cap as a failed publish.
func TestBrowserSiteErasureFailureBecomesTerminalAtTheAttemptCap(t *testing.T) {
	svc, store, eraser, _ := browserSiteFanout(browserSiteChild("org-1", maxErasureFanoutAttempts))
	eraser.failOrgs = map[string]error{"org-1": errors.New("org-core browser-site erasure endpoint returned status 500")}

	if _, err := svc.processClaimedErasure(context.Background(), browserSiteOperation()); err == nil {
		t.Fatal("a failed erase must fail the fan-out")
	}
	if len(store.failTerminal) != 1 || !store.failTerminal[0] {
		t.Fatalf("the child at the attempt cap must be terminal, got %v", store.failTerminal)
	}
}
