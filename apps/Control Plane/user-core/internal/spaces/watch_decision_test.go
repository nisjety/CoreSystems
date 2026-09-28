package spaces

import (
	"strings"
	"testing"
	"time"
)

// A shared room, because that is the interesting case: the personal branch is
// the same authority every personal decision already uses.
func validWatchEvidence() PersonalThreadDecisionEvidence {
	return validSharedThreadReadEvidence()
}

const testPredicateDigest = "sha256:" +
	"1111111111111111111111111111111111111111111111111111111111111111"

func validWatchCreate() WatchCreateRequest {
	return WatchCreateRequest{
		DecisionRef: "watch-decision-1", WatchID: "wch_1",
		SourceKind: "process_output", SourceRef: "proc-1",
		PredicateDigest: testPredicateDigest,
		IdempotencyKey:  "watch-1", Nonce: "nonce-1",
	}
}

func validWatchObserve(evidence PersonalThreadDecisionEvidence) WatchObserveIntent {
	return WatchObserveIntent{
		OrgID: evidence.Membership.OrgID, SpaceRef: evidence.Membership.SpaceRef,
		SubjectID: evidence.Membership.SubjectID, WatchID: "wch_1",
		SourceKind: "process_output", SourceRef: "proc-1",
		PredicateDigest: testPredicateDigest, IdempotencyKey: "wch_1:12",
	}
}

func TestIssueWatchCreateDecisionIsTargetBoundAndGrantsOnlyCreate(t *testing.T) {
	now := time.Date(2026, 9, 14, 12, 0, 0, 0, time.UTC)
	decision, err := IssueWatchCreateDecision(validWatchEvidence(), validWatchCreate(), now)
	if err != nil {
		t.Fatalf("IssueWatchCreateDecision: %v", err)
	}
	if decision.ActionID != watchCreateAction || decision.ActionSchemaHash != watchCreateSchema {
		t.Fatalf("incorrect target: %+v", decision)
	}
	if len(decision.Permissions) != 1 || decision.Permissions[0] != "watch:create" {
		t.Fatalf("permissions = %#v; a create token must not also observe", decision.Permissions)
	}
	if decision.ServiceAudience != watchCreateAudience {
		t.Fatalf("audience = %q", decision.ServiceAudience)
	}
	if decision.RecipientAudienceRevision != 4 {
		t.Fatalf("the decision must carry the caller's current audience revision, got %d", decision.RecipientAudienceRevision)
	}
}

// A viewer may watch. A watch OBSERVES and creates no effect in the Space, and
// reading the shared record is what a viewer role is for — the thread-read
// decision already says so. Schedule creation requires editor because a
// schedule performs work; a watch does not.
func TestAViewerMayCreateAWatchEvenThoughAViewerCannotSchedule(t *testing.T) {
	evidence := validWatchEvidence()
	if evidence.Membership.Role != "viewer" {
		t.Fatalf("fixture role = %q, want viewer", evidence.Membership.Role)
	}
	if _, err := IssueWatchCreateDecision(evidence, validWatchCreate(), time.Now().UTC()); err != nil {
		t.Fatalf("a viewer was refused a watch: %v", err)
	}
	// The contrast that makes the rule legible: the same evidence cannot create
	// a schedule.
	if _, err := IssueScheduleCreateDecision(evidence, ScheduleCreateRequest{
		DecisionRef: "d", ScheduleID: "s", TemplateDigest: testPredicateDigest,
		IdempotencyKey: "i", Nonce: "n",
	}, time.Now().UTC()); err == nil {
		t.Fatal("a viewer created a schedule; the two authorities are meant to differ")
	}
}

// A watch is durable by construction — it stores a summary of what it saw on
// the row and in its event log. That is content derived from Space content,
// kept after the turn that produced it.
func TestZeroDataRetentionForbidsAWatch(t *testing.T) {
	evidence := validWatchEvidence()
	evidence.Privacy.ZeroDataRetention = true
	if _, err := IssueWatchCreateDecision(evidence, validWatchCreate(), time.Now().UTC()); err == nil {
		t.Fatal("a ZDR Space was allowed to create a durable watch")
	}
	if _, err := IssueWatchObserveDecision(evidence, validWatchObserve(evidence), "d", "n", time.Now().UTC()); err == nil {
		t.Fatal("a ZDR Space was allowed to record a watch observation")
	}
}

// The whole point of two actions: a creator's token must not serve as a
// standing worker grant after a role, audience or policy change.
func TestACreateDecisionCannotSatisfyAnObserveCheck(t *testing.T) {
	now := time.Date(2026, 9, 14, 12, 0, 0, 0, time.UTC)
	evidence := validWatchEvidence()
	create, err := IssueWatchCreateDecision(evidence, validWatchCreate(), now)
	if err != nil {
		t.Fatalf("create: %v", err)
	}
	observe, err := IssueWatchObserveDecision(evidence, validWatchObserve(evidence), "watch-decision-2", "nonce-2", now)
	if err != nil {
		t.Fatalf("observe: %v", err)
	}
	if create.ActionID == observe.ActionID || create.ActionSchemaHash == observe.ActionSchemaHash {
		t.Fatal("the two actions are indistinguishable; a create token could be replayed to observe")
	}
	if create.PayloadDigest == observe.PayloadDigest {
		// The action and schema are digest inputs precisely so this cannot
		// happen even when every other bound fact is identical.
		t.Fatal("the two digests collide despite different actions")
	}
	if len(observe.Permissions) != 1 || observe.Permissions[0] != "watch:observe" {
		t.Fatalf("observe permissions = %#v", observe.Permissions)
	}
}

// The predicate is bound by digest so it cannot be swapped after approval — a
// watch approved for "tell me when it says ERROR" must not become "tell me
// everything", which for a watch is the difference between a notification and a
// transcript.
func TestThePredicateDigestIsBoundIntoTheDecision(t *testing.T) {
	now := time.Date(2026, 9, 14, 12, 0, 0, 0, time.UTC)
	evidence := validWatchEvidence()
	first, err := IssueWatchCreateDecision(evidence, validWatchCreate(), now)
	if err != nil {
		t.Fatalf("first: %v", err)
	}
	swapped := validWatchCreate()
	swapped.PredicateDigest = "sha256:" + strings.Repeat("2", 64)
	second, err := IssueWatchCreateDecision(evidence, swapped, now)
	if err != nil {
		t.Fatalf("second: %v", err)
	}
	if first.PayloadDigest == second.PayloadDigest {
		t.Fatal("changing the predicate did not change the payload digest")
	}
}

// The source is bound too: an approved watch on one process must not be
// redirected at another.
func TestTheSourceIsBoundIntoTheDecision(t *testing.T) {
	now := time.Date(2026, 9, 14, 12, 0, 0, 0, time.UTC)
	evidence := validWatchEvidence()
	first, err := IssueWatchCreateDecision(evidence, validWatchCreate(), now)
	if err != nil {
		t.Fatalf("first: %v", err)
	}
	elsewhere := validWatchCreate()
	elsewhere.SourceRef = "proc-2"
	second, err := IssueWatchCreateDecision(evidence, elsewhere, now)
	if err != nil {
		t.Fatalf("second: %v", err)
	}
	if first.PayloadDigest == second.PayloadDigest {
		t.Fatal("changing the watched source did not change the payload digest")
	}
}

// Nothing in the intent is taken on trust: the sweeper names an org, Space and
// subject, and Control checks them against the authority it resolved itself.
func TestAnObserveIntentMustMatchCurrentAuthority(t *testing.T) {
	now := time.Date(2026, 9, 14, 12, 0, 0, 0, time.UTC)
	evidence := validWatchEvidence()
	for name, mutate := range map[string]func(*WatchObserveIntent){
		"another org":     func(i *WatchObserveIntent) { i.OrgID = "org-other" },
		"another Space":   func(i *WatchObserveIntent) { i.SpaceRef = "room-other" },
		"another subject": func(i *WatchObserveIntent) { i.SubjectID = "user-other" },
	} {
		intent := validWatchObserve(evidence)
		mutate(&intent)
		if _, err := IssueWatchObserveDecision(evidence, intent, "d", "n", now); err == nil {
			t.Fatalf("an intent naming %s was signed", name)
		}
	}
}

func TestWatchRequestsValidateTheirOwnFields(t *testing.T) {
	now := time.Date(2026, 9, 14, 12, 0, 0, 0, time.UTC)
	evidence := validWatchEvidence()
	for name, mutate := range map[string]func(*WatchCreateRequest){
		"no watch id":      func(r *WatchCreateRequest) { r.WatchID = "" },
		"no source kind":   func(r *WatchCreateRequest) { r.SourceKind = " " },
		"no source ref":    func(r *WatchCreateRequest) { r.SourceRef = "" },
		"no digest":        func(r *WatchCreateRequest) { r.PredicateDigest = "" },
		"malformed digest": func(r *WatchCreateRequest) { r.PredicateDigest = "not-a-digest" },
		"short digest":     func(r *WatchCreateRequest) { r.PredicateDigest = "sha256:abc" },
		"no nonce":         func(r *WatchCreateRequest) { r.Nonce = "" },
	} {
		request := validWatchCreate()
		mutate(&request)
		if _, err := IssueWatchCreateDecision(evidence, request, now); err == nil {
			t.Fatalf("a create request with %s was signed", name)
		}
	}
}

// A member with no current membership has no authority to observe, however old
// the watch is. The long-lived record is not the authority.
func TestARevokedMemberCannotObserve(t *testing.T) {
	evidence := validWatchEvidence()
	evidence.Membership.Role = ""
	if _, err := IssueWatchObserveDecision(evidence, validWatchObserve(evidence), "d", "n", time.Now().UTC()); err == nil {
		t.Fatal("a member with no role was allowed to observe")
	}
}
