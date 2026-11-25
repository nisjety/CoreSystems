package api

import (
	"bytes"
	"container/list"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"sync"
	"time"

	"github.com/triodelab/model-plane/services/capability-core/internal/decisionkey"
)

// F-f10-19 (D3-1151): capability-core verifies the Control-signed model.space.skill.write decision
// user-core issues (F-f10-18, SPACE_SKILL_WRITE_DECISIONS) before a Space skill is created, changed or
// deleted. It is modelled on cron.ControlDecisionVerifier and holds only Control's public key, from
// the same CONTROL_SPACE_DECISION_KEY_ID / CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64 configuration.
//
// Nothing here trusts a Space role from the client or the BFF (EP-11 D7): the role is Control's call,
// signed into the decision. What capability-core checks is that the decision binds this exact write:
// the verified org and person, the Space (the body's for a create, the stored row's for a change), the
// operation, the skill id and the digest of the body it received. The token is never logged or stored.

const (
	// spaceSkillDecisionHeader carries the signed decision the BFF obtained from Control.
	spaceSkillDecisionHeader = "X-Space-Skill-Decision"
	// skillSpaceWriteVerifyEnv is SKILL_SPACE_WRITE_VERIFY. Turn it on only together with
	// SKILL_SPACE_SCOPE here and SPACE_SKILL_WRITE_DECISIONS on user-core.
	skillSpaceWriteVerifyEnv = "SKILL_SPACE_WRITE_VERIFY"

	spaceSkillDecisionVersion  = "v2"
	spaceSkillWriteAction      = "model.space.skill.write"
	spaceSkillWriteSchema      = "sha256:space-skill-write-v1"
	spaceSkillWriteAudience    = "model-plane-capability-core"
	spaceSkillWritePermission  = "skill:write"
	spaceSkillOperationCreate  = "create"
	spaceSkillOperationUpdate  = "update"
	spaceSkillOperationDelete  = "delete"
	spaceSkillDigestPrefix     = "sha256:"
	spaceSkillDecisionLifetime = 60 * time.Second
	spaceSkillIssuedAtSkew     = 5 * time.Second
	spaceSkillNonceCapacity    = 4096

	// spaceSkillDecisionInvalidCode refuses a decision that does not verify or does not bind this write.
	spaceSkillDecisionInvalidCode = "space_skill_decision_invalid"
	// spaceSkillDecisionReplayedCode refuses a decision whose nonce this replica has already used.
	spaceSkillDecisionReplayedCode = "space_skill_decision_replayed"
)

var (
	// errSpaceSkillDecisionInvalid says only that the decision was refused, never which field, so a
	// refusal teaches a forger nothing.
	errSpaceSkillDecisionInvalid = errors.New("the Space skill write decision does not authorize this write")
	// errSpaceSkillDecisionReplayed is a nonce used before, inside the decision's lifetime.
	errSpaceSkillDecisionReplayed = errors.New("the Space skill write decision was already used")
	// errSpaceSkillDecisionZDR is a verified decision for a Zero Data Retention Space: a skill is
	// durable content, so it is refused with the catalogue's typed ZDR refusal (fails closed).
	errSpaceSkillDecisionZDR = errors.New("a Space skill cannot be written in a zero data retention Space")
)

// SkillSpaceWriteVerifyFromLookup reads SKILL_SPACE_WRITE_VERIFY strictly: only the literal "true"
// lets a verified decision authorize a Space skill write. Off, every such write is refused with
// space_skill_decision_required, as before.
func SkillSpaceWriteVerifyFromLookup(lookup func(string) string) bool {
	return lookup(skillSpaceWriteVerifyEnv) == "true"
}

// SkillWriteIntent is the write capability-core is about to make, built from the verified principal,
// the route and the received body or the stored row; never from a header or a body claim of authority.
type SkillWriteIntent struct {
	OrgID     string
	SpaceRef  string
	SubjectID string
	// Operation is create, update or delete.
	Operation string
	// SkillID is the target; empty for a create (capability-core mints the id).
	SkillID string
	// RequestDigest is spaceSkillRequestDigest of the body received.
	RequestDigest string
}

// spaceSkillDecision is the part of Control's decision envelope this verifier reads.
type spaceSkillDecision struct {
	DecisionRef               string    `json:"decision_ref"`
	OrgID                     string    `json:"org_id"`
	SpaceRef                  string    `json:"space_ref"`
	SubjectID                 string    `json:"subject_id"`
	ServiceAudience           string    `json:"service_audience"`
	ActionID                  string    `json:"action_id"`
	ActionSchemaHash          string    `json:"action_schema_hash"`
	PayloadDigest             string    `json:"payload_digest"`
	IdempotencyKey            string    `json:"idempotency_key"`
	RecipientAudienceRef      string    `json:"recipient_audience_ref"`
	RecipientAudienceHash     string    `json:"recipient_audience_hash"`
	PrivacyPolicyRef          string    `json:"privacy_policy_ref"`
	ResourceAuthorizationRef  string    `json:"resource_authorization_ref"`
	AuthorityRevision         int64     `json:"authority_revision"`
	RecipientAudienceRevision int64     `json:"recipient_audience_revision"`
	Permissions               []string  `json:"permissions"`
	ZeroDataRetention         bool      `json:"zero_data_retention"`
	IssuedAt                  time.Time `json:"issued_at"`
	ExpiresAt                 time.Time `json:"expires_at"`
	Nonce                     string    `json:"nonce"`
}

// SpaceSkillDecisionVerifier verifies Control's public, key-identified Space skill write decision and
// keeps the nonces it has accepted, so a decision is single-use on this replica. Another replica
// cannot see them; the binding to the body digest, the skill and the operation is the main control.
type SpaceSkillDecisionVerifier struct {
	keyID  string
	public ed25519.PublicKey
	now    func() time.Time

	mu     sync.Mutex
	nonces map[string]*list.Element
	order  *list.List // of spaceSkillNonce, oldest first
}

type spaceSkillNonce struct {
	nonce     string
	expiresAt time.Time
}

// NewSpaceSkillDecisionVerifier requires the Control key id and public key explicitly. There is no
// development key: a missing key leaves every Space skill write refused.
func NewSpaceSkillDecisionVerifier(keyID, publicKeyBase64 string) (*SpaceSkillDecisionVerifier, error) {
	keyID = strings.TrimSpace(keyID)
	publicRaw, _, err := decisionkey.DecodePublicKey(publicKeyBase64)
	if keyID == "" || err != nil {
		return nil, fmt.Errorf("Control Space skill write decision public key is invalid")
	}
	return &SpaceSkillDecisionVerifier{
		keyID: keyID, public: ed25519.PublicKey(publicRaw), now: time.Now,
		nonces: make(map[string]*list.Element), order: list.New(),
	}, nil
}

// Verify accepts token only if Control signed it with the configured key for exactly want, it is
// current, and its nonce is unused here. Every refusal is errSpaceSkillDecisionInvalid except a used
// nonce (errSpaceSkillDecisionReplayed) and a verified decision for a ZDR Space (errSpaceSkillDecisionZDR).
func (v *SpaceSkillDecisionVerifier) Verify(token string, want SkillWriteIntent) error {
	if v == nil || !validSkillWriteIntent(want) {
		return errSpaceSkillDecisionInvalid
	}
	decision, err := v.parse(token)
	if err != nil {
		return errSpaceSkillDecisionInvalid
	}
	now := v.now().UTC()
	if decision.ServiceAudience != spaceSkillWriteAudience || decision.ActionID != spaceSkillWriteAction ||
		decision.ActionSchemaHash != spaceSkillWriteSchema ||
		len(decision.Permissions) != 1 || decision.Permissions[0] != spaceSkillWritePermission ||
		decision.OrgID != want.OrgID || decision.SpaceRef != want.SpaceRef || decision.SubjectID != want.SubjectID ||
		strings.TrimSpace(decision.DecisionRef) == "" || strings.TrimSpace(decision.IdempotencyKey) == "" ||
		strings.TrimSpace(decision.Nonce) == "" {
		return errSpaceSkillDecisionInvalid
	}
	if decision.IssuedAt.IsZero() || decision.IssuedAt.After(now.Add(spaceSkillIssuedAtSkew)) ||
		!now.Before(decision.ExpiresAt) || !decision.IssuedAt.Before(decision.ExpiresAt) ||
		decision.ExpiresAt.Sub(decision.IssuedAt) > spaceSkillDecisionLifetime {
		return errSpaceSkillDecisionInvalid
	}
	if decision.PayloadDigest != spaceSkillWritePayloadDigest(decision, want) {
		return errSpaceSkillDecisionInvalid
	}
	if decision.ZeroDataRetention {
		return errSpaceSkillDecisionZDR
	}
	if !v.useNonce(decision.Nonce, decision.ExpiresAt, now) {
		return errSpaceSkillDecisionReplayed
	}
	return nil
}

func validSkillWriteIntent(want SkillWriteIntent) bool {
	if want.OrgID == "" || want.SpaceRef == "" || want.SubjectID == "" || !validSpaceSkillRequestDigest(want.RequestDigest) {
		return false
	}
	switch want.Operation {
	case spaceSkillOperationCreate:
		return want.SkillID == ""
	case spaceSkillOperationUpdate, spaceSkillOperationDelete:
		return want.SkillID != ""
	}
	return false
}

// parse checks the envelope version, key id and signature and decodes the payload.
func (v *SpaceSkillDecisionVerifier) parse(token string) (spaceSkillDecision, error) {
	parts := strings.Split(strings.TrimSpace(token), ".")
	if len(parts) != 4 || parts[0] != spaceSkillDecisionVersion {
		return spaceSkillDecision{}, errSpaceSkillDecisionInvalid
	}
	keyID, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil || string(keyID) != v.keyID {
		if err == nil {
			decisionkey.ReportUnknownKey("space-skill-write", string(keyID), v.keyID)
		}
		return spaceSkillDecision{}, errSpaceSkillDecisionInvalid
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[3])
	if err != nil || !ed25519.Verify(v.public, []byte(strings.Join(parts[:3], ".")), signature) {
		return spaceSkillDecision{}, errSpaceSkillDecisionInvalid
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		return spaceSkillDecision{}, errSpaceSkillDecisionInvalid
	}
	var decision spaceSkillDecision
	if err := json.Unmarshal(payload, &decision); err != nil {
		return spaceSkillDecision{}, errSpaceSkillDecisionInvalid
	}
	return decision, nil
}

// useNonce records nonce until expiresAt and reports whether it was unused. Expired nonces are
// dropped first; past spaceSkillNonceCapacity the oldest is evicted.
func (v *SpaceSkillDecisionVerifier) useNonce(nonce string, expiresAt, now time.Time) bool {
	v.mu.Lock()
	defer v.mu.Unlock()
	for front := v.order.Front(); front != nil; front = v.order.Front() {
		entry := front.Value.(spaceSkillNonce)
		if now.Before(entry.expiresAt) {
			break
		}
		v.order.Remove(front)
		delete(v.nonces, entry.nonce)
	}
	if _, used := v.nonces[nonce]; used {
		return false
	}
	for v.order.Len() >= spaceSkillNonceCapacity {
		oldest := v.order.Front()
		v.order.Remove(oldest)
		delete(v.nonces, oldest.Value.(spaceSkillNonce).nonce)
	}
	v.nonces[nonce] = v.order.PushBack(spaceSkillNonce{nonce: nonce, expiresAt: expiresAt})
	return true
}

// spaceSkillWritePayloadDigest mirrors user-core's spaceSkillWritePayloadDigest (F-f10-18) byte for
// byte: the prefix, then each (name, value) as name + 0x00 + u64 big-endian length + value, then the
// authority and recipient-audience revisions as name + 0x00 + u64 big-endian. The operation, skill id
// and request digest come from the write capability-core is making, never from the decision.
func spaceSkillWritePayloadDigest(decision spaceSkillDecision, want SkillWriteIntent) string {
	hash := sha256.New()
	hash.Write([]byte("model.space.skill.write\x00v1\x00"))
	for _, field := range []struct{ name, value string }{
		{"org_id", decision.OrgID},
		{"user_id", decision.SubjectID},
		{"space_id", decision.SpaceRef},
		{"space_decision_ref", decision.DecisionRef},
		{"recipient_audience_ref", decision.RecipientAudienceRef},
		{"recipient_audience_hash", decision.RecipientAudienceHash},
		{"privacy_policy_ref", decision.PrivacyPolicyRef},
		{"resource_authorization_ref", decision.ResourceAuthorizationRef},
		{"operation", want.Operation},
		{"skill_id", want.SkillID},
		{"request_digest", want.RequestDigest},
		{"action_schema_hash", spaceSkillWriteSchema},
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
	return spaceSkillDigestPrefix + hex.EncodeToString(hash.Sum(nil))
}

// spaceSkillRequestDigest is the request_digest a decision must bind: "sha256:" + lowercase hex of the
// canonical body. A delete's body is empty, so its digest is that of zero bytes whatever was sent. A
// create or update body is canonicalised from the first JSON value received: object keys sorted, no
// insignificant whitespace, no HTML escaping, numbers exactly as sent, no trailing newline. The BFF
// hashes the same canonical form of the body it forwards.
func spaceSkillRequestDigest(operation string, body []byte) (string, error) {
	if operation == spaceSkillOperationDelete {
		sum := sha256.Sum256(nil)
		return spaceSkillDigestPrefix + hex.EncodeToString(sum[:]), nil
	}
	canonical, err := canonicalSpaceSkillBody(body)
	if err != nil {
		return "", err
	}
	sum := sha256.Sum256(canonical)
	return spaceSkillDigestPrefix + hex.EncodeToString(sum[:]), nil
}

func canonicalSpaceSkillBody(body []byte) ([]byte, error) {
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.UseNumber()
	var value any
	if err := decoder.Decode(&value); err != nil {
		return nil, errSpaceSkillDecisionInvalid
	}
	var buffer bytes.Buffer
	encoder := json.NewEncoder(&buffer)
	encoder.SetEscapeHTML(false)
	if err := encoder.Encode(value); err != nil {
		return nil, errSpaceSkillDecisionInvalid
	}
	return bytes.TrimRight(buffer.Bytes(), "\n"), nil
}

func validSpaceSkillRequestDigest(value string) bool {
	hexPart, ok := strings.CutPrefix(value, spaceSkillDigestPrefix)
	if !ok || len(hexPart) != sha256.Size*2 {
		return false
	}
	for _, r := range hexPart {
		if (r < '0' || r > '9') && (r < 'a' || r > 'f') {
			return false
		}
	}
	return true
}
