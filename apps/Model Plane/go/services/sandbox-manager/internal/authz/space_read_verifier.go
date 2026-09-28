package authz

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"strings"
	"time"
)

// The Control-signed decision that admits a HUMAN to a Space's background
// processes (S4.2 §7).
//
// # Why this is the model.thread.read decision and not a new one
//
// The Work tab already obtains a `model.thread.read` decision to list a
// Space's runs, and Session Core already verifies it. A process is part of the
// same record as a run — it is work this Space did, its output is content in
// the same sense a turn is — so the question a reader is asking is unchanged.
// Minting a `model.process.read` action would have created a second authority
// to keep in sync with the first, and a Space where a member could read the
// conversation but not the work it produced is not a state anyone would choose.
//
// What DOES change is the audience: Control signs this one for
// `model-plane-sandbox-manager` instead of `model-plane`. That makes the two
// non-interchangeable — Session Core refuses a decision addressed here and
// this verifier refuses one addressed there — so a token leaked from one path
// cannot be replayed against the other.
//
// # Why this is a local copy rather than a shared package
//
// Same reason SpaceCapabilityVerifier in this package is: sandbox-manager and
// user-core are independently deployed Go modules with no shared package for
// this envelope, and Session Core's copy is in Rust. The envelope parsing
// (version, key id, signature) is genuinely shared with the capability
// verifier and reused from it; only the claims and the checks differ.
const (
	spaceReadAction   = "model.thread.read"
	spaceReadAudience = "model-plane-sandbox-manager"
	spaceReadSchema   = "sha256:thread-read-v1"
)

// spaceReadDecision mirrors Control's Decision envelope for a thread-read.
// Only the fields this verifier interprets are declared.
type spaceReadDecision struct {
	DecisionRef               string    `json:"decision_ref"`
	OrgID                     string    `json:"org_id"`
	SpaceRef                  string    `json:"space_ref"`
	SubjectID                 string    `json:"subject_id"`
	ServiceAudience           string    `json:"service_audience"`
	ActionID                  string    `json:"action_id"`
	ActionSchemaHash          string    `json:"action_schema_hash"`
	PayloadDigest             string    `json:"payload_digest"`
	IdempotencyKey            string    `json:"idempotency_key"`
	Nonce                     string    `json:"nonce"`
	RecipientAudienceRef      string    `json:"recipient_audience_ref"`
	RecipientAudienceHash     string    `json:"recipient_audience_hash"`
	PrivacyPolicyRef          string    `json:"privacy_policy_ref"`
	ResourceAuthorizationRef  string    `json:"resource_authorization_ref"`
	Purpose                   string    `json:"purpose"`
	LawfulBasis               string    `json:"lawful_basis"`
	PrivacyClass              string    `json:"privacy_class"`
	RetentionClass            string    `json:"retention_class"`
	Residency                 string    `json:"residency"`
	DeletionScope             string    `json:"deletion_scope"`
	AuthorityRevision         int64     `json:"authority_revision"`
	RecipientAudienceRevision int64     `json:"recipient_audience_revision"`
	Permissions               []string  `json:"permissions"`
	IssuedAt                  time.Time `json:"issued_at"`
	ExpiresAt                 time.Time `json:"expires_at"`
}

// SpaceReadExpectation is the request-derived context a verified read decision
// must match. Nothing here comes from the token: org and subject come from the
// caller's authenticated identity, and the Space from the request.
type SpaceReadExpectation struct {
	OrgID       string
	SpaceRef    string
	SubjectID   string
	DecisionRef string
	Now         time.Time
}

// VerifiedSpaceRead is what a caller may trust after VerifySpaceRead: the
// audience ceiling every row returned under this decision must satisfy.
//
// Only the ceiling, deliberately. A read decision grants exactly one thing,
// and returning a richer struct would invite a caller to find some other use
// for a token that was issued for this one.
type VerifiedSpaceRead struct {
	RecipientAudienceRevision int64
}

// VerifySpaceRead checks the envelope, then every condition Session Core's
// verify_thread_read_space_decision checks, in the same order and for the same
// reasons (session-core/src/grpc.rs).
//
// The port is deliberate rather than approximate: two services enforcing
// "different but similar" versions of one authority is how a hole opens that
// neither side's tests can see. Where this differs from Session Core it
// differs on purpose, and there is exactly one such place — the audience.
func (v *SpaceCapabilityVerifier) VerifySpaceRead(token string, expect SpaceReadExpectation) (VerifiedSpaceRead, error) {
	if strings.TrimSpace(token) == "" || strings.TrimSpace(expect.DecisionRef) == "" || strings.TrimSpace(expect.SpaceRef) == "" {
		return VerifiedSpaceRead{}, fmt.Errorf("a Space read decision requires space_id, decision ref, and token together")
	}
	if len(token) > maxSpaceReadTokenBytes {
		return VerifiedSpaceRead{}, fmt.Errorf("Space read decision is too large")
	}
	payload, err := v.verifyEnvelope(token)
	if err != nil {
		return VerifiedSpaceRead{}, err
	}
	var decision spaceReadDecision
	if err := json.Unmarshal(payload, &decision); err != nil {
		return VerifiedSpaceRead{}, fmt.Errorf("invalid Space read decision claims")
	}

	matches := decision.DecisionRef == expect.DecisionRef &&
		decision.OrgID == expect.OrgID &&
		decision.SubjectID == expect.SubjectID &&
		decision.SpaceRef == expect.SpaceRef &&
		decision.ServiceAudience == spaceReadAudience &&
		decision.ActionID == spaceReadAction &&
		decision.ActionSchemaHash == spaceReadSchema &&
		decision.PayloadDigest == expectedSpaceReadPayloadDigest(decision) &&
		hasPermission(decision.Permissions, "thread:read")
	if !matches {
		return VerifiedSpaceRead{}, fmt.Errorf("Space read decision does not authorize this process read")
	}
	// A read decision must never double as authority to write. Control issues
	// disjoint permission sets; checking it here keeps that disjointness a
	// property the reader enforces rather than one it takes on trust.
	if hasPermission(decision.Permissions, "thread:append") || hasPermission(decision.Permissions, "thread:create") {
		return VerifiedSpaceRead{}, fmt.Errorf("a Space read decision must not carry write permissions")
	}

	now := expect.Now
	if now.IsZero() {
		now = time.Now().UTC()
	}
	privacyComplete := true
	for _, value := range []string{
		decision.Purpose, decision.LawfulBasis, decision.PrivacyClass,
		decision.RetentionClass, decision.Residency, decision.DeletionScope,
	} {
		if strings.TrimSpace(value) == "" {
			privacyComplete = false
		}
	}
	if !privacyComplete ||
		strings.TrimSpace(decision.Nonce) == "" ||
		strings.TrimSpace(decision.RecipientAudienceRef) == "" ||
		decision.RecipientAudienceRevision == 0 ||
		decision.IssuedAt.After(now.Add(time.Minute)) ||
		!decision.ExpiresAt.After(now) {
		return VerifiedSpaceRead{}, fmt.Errorf("Space read decision is expired or incomplete")
	}
	return VerifiedSpaceRead{RecipientAudienceRevision: decision.RecipientAudienceRevision}, nil
}

// maxSpaceReadTokenBytes mirrors session-core's own cap. A decision is a small
// signed JSON object; anything approaching this is either corrupt or an
// attempt to make the verifier do unbounded work before it rejects.
const maxSpaceReadTokenBytes = 8192

// expectedSpaceReadPayloadDigest mirrors Control's
// sharedThreadReadPayloadDigest (user-core/internal/spaces/
// shared_thread_read_decision.go) field for field, length-prefixed the same
// way.
//
// Note what is absent: the service audience. It is a signed field of the
// decision, so a tampered audience fails the signature check before this runs
// — and Control deliberately left it out so that adding a second audience did
// not change the digest Session Core independently recomputes for the first.
func expectedSpaceReadPayloadDigest(decision spaceReadDecision) string {
	hash := sha256.New()
	hash.Write([]byte("model.thread.read\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", decision.OrgID},
		{"user_id", decision.SubjectID},
		{"space_id", decision.SpaceRef},
		{"space_decision_ref", decision.DecisionRef},
		{"recipient_audience_ref", decision.RecipientAudienceRef},
		{"recipient_audience_hash", decision.RecipientAudienceHash},
		{"privacy_policy_ref", decision.PrivacyPolicyRef},
		{"resource_authorization_ref", decision.ResourceAuthorizationRef},
		{"action_schema_hash", spaceReadSchema},
		{"idempotency_key", decision.IdempotencyKey},
	} {
		hash.Write([]byte(field.name))
		hash.Write([]byte{0})
		var length [8]byte
		binary.BigEndian.PutUint64(length[:], uint64(len(field.value)))
		hash.Write(length[:])
		hash.Write([]byte(field.value))
	}
	for _, revision := range []struct {
		name  string
		value int64
	}{
		{"authority_revision", decision.AuthorityRevision},
		{"recipient_audience_revision", decision.RecipientAudienceRevision},
	} {
		hash.Write([]byte(revision.name))
		hash.Write([]byte{0})
		var encoded [8]byte
		binary.BigEndian.PutUint64(encoded[:], uint64(revision.value))
		hash.Write(encoded[:])
	}
	return "sha256:" + hex.EncodeToString(hash.Sum(nil))
}
