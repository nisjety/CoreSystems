package spaces

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"strings"
	"time"
)

// The S4.3 Watch decision pair.
//
// Two actions rather than one, for the reason the schedule pair gives: a
// creator's token must not serve as a standing worker grant. A watch created
// weeks ago under a membership since revoked, or a Space whose recipient
// audience has moved, must not keep disclosing — so the sweeper re-derives
// current authority before it records anything, and a create token cannot be
// replayed to do it.
const (
	watchCreateAction   = "model.watch.create"
	watchCreateAudience = "model-plane-capability-core"
	watchCreateSchema   = "sha256:space-watch-create-v1"

	watchObserveAction   = "model.watch.observe"
	watchObserveAudience = "model-plane-capability-core"
	watchObserveSchema   = "sha256:space-watch-observe-v1"
)

// WatchCreateRequest binds Control's authorization to the watch id, what it
// watches, and a content-free digest of its predicate.
//
// The predicate travels as a DIGEST for the same reason a task template does:
// Control does not need the matching rule to enforce Space policy, and binding
// the digest means the Model Plane cannot swap in a different predicate after
// approval. A watch approved for "tell me when it says ERROR" must not become
// "tell me everything" — which, for a watch, is the difference between a
// notification and a transcript.
type WatchCreateRequest struct {
	DecisionRef     string
	WatchID         string
	SourceKind      string
	SourceRef       string
	PredicateDigest string
	IdempotencyKey  string
	Nonce           string
}

func (r WatchCreateRequest) Validate() error {
	for label, value := range map[string]string{
		"decision_ref": r.DecisionRef, "watch_id": r.WatchID,
		"source_kind": r.SourceKind, "source_ref": r.SourceRef,
		"predicate_digest": r.PredicateDigest, "idempotency_key": r.IdempotencyKey,
		"nonce": r.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("watch create decision %s is required", label)
		}
	}
	if !isSHA256Digest(r.PredicateDigest) {
		return fmt.Errorf("watch create predicate digest is invalid")
	}
	return nil
}

// WatchObserveIntent is the non-secret, immutable watch record view the sweeper
// presents to Control immediately before it records anything.
//
// Deliberately excludes any prior decision and all mutable authority data:
// Control re-resolves those from its own store on every attempt. It also
// excludes the matched content — Control authorizes the disclosure, it does not
// review it, and sending a program's output to the identity plane would put
// unscreened payload somewhere it has no business being.
type WatchObserveIntent struct {
	OrgID           string
	SpaceRef        string
	SubjectID       string
	WatchID         string
	SourceKind      string
	SourceRef       string
	PredicateDigest string
	IdempotencyKey  string
}

func (i WatchObserveIntent) Validate() error {
	for label, value := range map[string]string{
		"org_id": i.OrgID, "space_ref": i.SpaceRef, "subject_id": i.SubjectID,
		"watch_id": i.WatchID, "source_kind": i.SourceKind, "source_ref": i.SourceRef,
		"predicate_digest": i.PredicateDigest, "idempotency_key": i.IdempotencyKey,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("watch observe intent %s is required", label)
		}
	}
	if !isSHA256Digest(i.PredicateDigest) {
		return fmt.Errorf("watch observe intent predicate digest is invalid")
	}
	return nil
}

func isSHA256Digest(value string) bool {
	return strings.HasPrefix(value, "sha256:") && len(value) == len("sha256:")+64
}

func watchCreatePayloadDigest(evidence PersonalThreadDecisionEvidence, request WatchCreateRequest) string {
	return watchPayloadDigest(watchCreateAction, watchCreateSchema, watchDigestInput{
		OrgID: evidence.Membership.OrgID, SubjectID: evidence.Membership.SubjectID,
		SpaceRef: evidence.Membership.SpaceRef, WatchID: request.WatchID,
		SourceKind: request.SourceKind, SourceRef: request.SourceRef,
		PredicateDigest: request.PredicateDigest, IdempotencyKey: request.IdempotencyKey,
	}, evidence)
}

func watchObservePayloadDigest(evidence PersonalThreadDecisionEvidence, intent WatchObserveIntent) string {
	return watchPayloadDigest(watchObserveAction, watchObserveSchema, watchDigestInput{
		OrgID: evidence.Membership.OrgID, SubjectID: evidence.Membership.SubjectID,
		SpaceRef: evidence.Membership.SpaceRef, WatchID: intent.WatchID,
		SourceKind: intent.SourceKind, SourceRef: intent.SourceRef,
		PredicateDigest: intent.PredicateDigest, IdempotencyKey: intent.IdempotencyKey,
	}, evidence)
}

type watchDigestInput struct {
	OrgID           string
	SubjectID       string
	SpaceRef        string
	WatchID         string
	SourceKind      string
	SourceRef       string
	PredicateDigest string
	IdempotencyKey  string
}

// watchPayloadDigest is shared by both actions because they bind the same
// facts; the action id and schema hash are inputs, so the two digests can never
// collide and a create token can never satisfy an observe check.
//
// Every field is length-prefixed, matching every other Space decision digest in
// this repo, for the same collision-resistance reason.
func watchPayloadDigest(action, schema string, in watchDigestInput, evidence PersonalThreadDecisionEvidence) string {
	hash := sha256.New()
	hash.Write([]byte(action + "\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", in.OrgID},
		{"user_id", in.SubjectID},
		{"space_id", in.SpaceRef},
		{"watch_id", in.WatchID},
		{"source_kind", in.SourceKind},
		{"source_ref", in.SourceRef},
		{"predicate_digest", in.PredicateDigest},
		{"recipient_audience_ref", evidence.RecipientAudienceRef},
		{"recipient_audience_hash", evidence.RecipientAudienceHash},
		{"privacy_policy_ref", evidence.Privacy.PolicyRef},
		{"resource_authorization_ref", evidence.ResourceAuthorizationRef},
		{"action_schema_hash", schema},
		{"idempotency_key", in.IdempotencyKey},
	} {
		hash.Write([]byte(field.name))
		hash.Write([]byte{0})
		var length [8]byte
		binary.BigEndian.PutUint64(length[:], uint64(len(field.value)))
		hash.Write(length[:])
		hash.Write([]byte(field.value))
	}
	writeDigestRevisions(hash,
		digestRevision{"authority_revision", evidence.Membership.Revisions.Authority},
		digestRevision{"membership_revision", evidence.Membership.Revisions.Membership},
		digestRevision{"privacy_revision", evidence.Membership.Revisions.Privacy},
		digestRevision{"recipient_audience_revision", evidence.Membership.Revisions.RecipientAudience},
		digestRevision{"entitlement_revision", evidence.Membership.Revisions.Entitlement},
	)
	return "sha256:" + hex.EncodeToString(hash.Sum(nil))
}

// validateWatchAuthority is the role and privacy floor both watch actions share.
//
// # Why a viewer may watch
//
// A watch OBSERVES; it creates no effect in the Space. Schedule creation
// requires editor or above because a schedule performs work. Reading the shared
// record is what a viewer role is for — the thread-read decision already says so
// in as many words, and a member who may read a room's conversation may
// certainly ask to be told when its build breaks.
//
// # Why ZDR forbids it
//
// A watch is durable by construction: it stores a summary of what it saw on the
// watch row and in its event log. That is content derived from Space content,
// kept after the turn that produced it — exactly what zero data retention
// forbids, and the same reason it forbids durable schedule creation.
//
// # Why there is no watch entitlement
//
// Stated rather than omitted. A watch observes work the Space is already doing,
// and the things that needed their own entitlement already have one: a
// background process cannot exist without `process_registry_entitled`, so a
// watch on a process a Space may not run has nothing to watch. Adding a second
// entitlement for the observation of an already-entitled effect would be a
// switch nobody could explain. If watch VOLUME turns out to need governing,
// that is a quota, not an authority.
func validateWatchAuthority(evidence PersonalThreadDecisionEvidence, verb string) error {
	// A watch is valid in BOTH kinds of Space — a person watching their own
	// build, or a room watching a shared one — so the authority check dispatches
	// on kind rather than picking one and refusing the other.
	//
	// Both branches already admit viewer and above, which is the role floor this
	// action wants; the duplicate check that used to sit here is gone so there
	// is one source of truth for who may read a Space.
	//
	// The shared branch additionally requires ThreadReadEntitled, and reusing it
	// is the point: a standing read of a room is still a read of that room. A
	// Space not entitled to shared reads at all should not be entitled to
	// STANDING ones, and inventing a second entitlement for the same disclosure
	// would be a switch nobody could explain against the first.
	if evidence.Membership.Kind == KindPersonal {
		if err := evidence.validatePersonalAuthority(); err != nil {
			return fmt.Errorf("cannot %s a watch: %w", verb, err)
		}
	} else if err := evidence.ValidateForSharedThreadRead(); err != nil {
		return fmt.Errorf("cannot %s a watch: %w", verb, err)
	}
	if evidence.Privacy.ZeroDataRetention {
		return fmt.Errorf("zero data retention forbids a durable watch")
	}
	return nil
}

// IssueWatchCreateDecision authorizes exactly one durable watch create.
//
// Distinct from observe so a creator's token cannot be replayed by the sweeper
// after a role, audience, or policy change — the same separation the schedule
// pair has, and for the same reason.
func IssueWatchCreateDecision(evidence PersonalThreadDecisionEvidence, request WatchCreateRequest, now time.Time) (Decision, error) {
	if err := validateWatchAuthority(evidence, "create"); err != nil {
		return Decision{}, err
	}
	if err := request.Validate(); err != nil {
		return Decision{}, err
	}
	if now.IsZero() {
		return Decision{}, fmt.Errorf("watch create decision issuance time is required")
	}
	return newEvidenceDecision(
		evidence, request.DecisionRef, watchCreateAudience, watchCreateAction, watchCreateSchema,
		watchCreatePayloadDigest(evidence, request), request.IdempotencyKey, request.Nonce,
		[]string{"watch:create"}, evidence.Privacy.ZeroDataRetention, now,
	), nil
}

// IssueWatchObserveDecision is a fresh, one-observation Model Plane decision.
//
// The sweeper obtains one immediately before it records anything a human can
// read. Membership, privacy, audience and resource access are all re-resolved
// here; the long-lived watch row is not authority by itself.
//
// Note what this does NOT authorize: reading. The sweeper's poll happens on
// capability-core's own service credential and is bound to the watch's Space by
// the adapter. What needs a fresh human-authority check is turning that read
// into a durable, disclosable record — so the check sits on the disclosure,
// which is also why a quiet watch costs Control nothing.
func IssueWatchObserveDecision(
	evidence PersonalThreadDecisionEvidence,
	intent WatchObserveIntent,
	decisionRef string,
	nonce string,
	now time.Time,
) (Decision, error) {
	if err := validateWatchAuthority(evidence, "observe"); err != nil {
		return Decision{}, err
	}
	if err := intent.Validate(); err != nil {
		return Decision{}, err
	}
	if intent.OrgID != evidence.Membership.OrgID ||
		intent.SpaceRef != evidence.Membership.SpaceRef ||
		intent.SubjectID != evidence.Membership.SubjectID {
		return Decision{}, fmt.Errorf("watch observe intent does not match current authority")
	}
	if strings.TrimSpace(decisionRef) == "" || strings.TrimSpace(nonce) == "" || now.IsZero() {
		return Decision{}, fmt.Errorf("watch observe decision fields are required")
	}
	return newEvidenceDecision(
		evidence, decisionRef, watchObserveAudience, watchObserveAction, watchObserveSchema,
		watchObservePayloadDigest(evidence, intent), intent.IdempotencyKey, nonce,
		[]string{"watch:observe"}, evidence.Privacy.ZeroDataRetention, now,
	), nil
}
