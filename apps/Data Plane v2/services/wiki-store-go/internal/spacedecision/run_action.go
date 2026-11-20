package spacedecision

import (
	"bytes"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"hash"
	"io"
	"slices"
	"strings"
	"time"
)

// RunEnvelopeVersion opens an agent decision: run-action-v1.<key id>.<payload>.<signature>. Control signs it with the same key
// set as the human v2 envelope and names a different version, so neither decision is accepted where the other is expected
// (apps/Control Plane/user-core/internal/spaces/run_action_decision.go, WS18-S1). Ed25519 covers the first three parts.
const RunEnvelopeVersion = "run-action-v1"

// The room canvas agent rows of the golden registry (WS19-S9). They must equal apps/Model Plane/api/contracts/
// space-self-api-registry.json, and every copy of it must stay byte-identical. A change needs the golden, its copies and
// the golden test together.
const (
	ActionRunCanvasRead  = "space.canvas.read"
	ActionRunCanvasWrite = "space.canvas.write"
	SchemaRunCanvasRead  = "sha256:7760a5a5ce90f1c50ad52b3bb68cfcb409a0d348b55eb2f066cd92e85474cac1"
	SchemaRunCanvasWrite = "sha256:9f0b5e21a35e9daf7b00b5cda023b71b598b169f28c4e112208f73b66a7d19e0"
)

// runPayloadDigestDomain opens the agent payload digest. It is the domain of mp-contracts' payload_digest (WS18-S1).
const runPayloadDigestDomain = "space-self-api/v1"

// RunClaims is the decision body Control signs for one agent call (run_action_decision.go, RunActionDecision). The JSON names
// match Control's tags, and an unknown field refuses the envelope as Control's own decoder does.
type RunClaims struct {
	DecisionRef                string    `json:"decision_ref"`
	RunID                      string    `json:"run_id"`
	ThreadID                   string    `json:"thread_id"`
	OrgID                      string    `json:"org_id"`
	SpaceRef                   string    `json:"space_ref"`
	SubjectID                  string    `json:"subject_id"`
	ServiceAudience            string    `json:"service_audience"`
	ActionID                   string    `json:"action_id"`
	ActionSchemaHash           string    `json:"action_schema_hash"`
	PayloadDigest              string    `json:"payload_digest"`
	IdempotencyKey             string    `json:"idempotency_key"`
	RecipientAudienceRef       string    `json:"recipient_audience_ref"`
	RecipientAudienceHash      string    `json:"recipient_audience_hash"`
	RecipientAudienceRevision  int64     `json:"recipient_audience_revision"`
	PrivacyPolicyRef           string    `json:"privacy_policy_ref"`
	RunContextAuthorizationRef string    `json:"run_context_authorization_ref"`
	AuthorityRevision          int64     `json:"authority_revision"`
	Permissions                []string  `json:"permissions"`
	Purpose                    string    `json:"purpose"`
	LawfulBasis                string    `json:"lawful_basis"`
	PrivacyClass               string    `json:"privacy_class"`
	ThirdPartyAllowed          bool      `json:"third_party_processing_allowed"`
	RetentionClass             string    `json:"retention_class"`
	Residency                  string    `json:"residency"`
	DeletionScope              string    `json:"deletion_scope"`
	ZeroDataRetention          bool      `json:"zero_data_retention"`
	IssuedAt                   time.Time `json:"issued_at"`
	ExpiresAt                  time.Time `json:"expires_at"`
	Nonce                      string    `json:"nonce"`
	// HistoryFloorAt (D18) is set only on a canvas read whose member has a timestamp window. Control refuses a revision floor
	// on a run action, so there is no revision claim here: an unknown field refuses the envelope, as Control's own decoder does.
	HistoryFloorAt *time.Time `json:"history_floor_at,omitempty"`
}

// RunArgument is one flattened argument of an agent call, in the row's argument_order. Absent optional text flattens to the
// empty string, and a null expected version flattens to the empty string as well.
type RunArgument struct {
	Name  string
	Value string
}

// VerifyRunAction checks a run-action-v1 envelope: its shape, key id, Ed25519 signature, the payload's completeness and its
// life. It binds no route; CheckCanvas binds the route and RunPayloadDigest binds the arguments. Every failure is
// space_decision_invalid, as for the human envelope.
func VerifyRunAction(token string, keys KeySet, now time.Time) (RunClaims, error) {
	if token == "" || len(token) > MaxEnvelopeBytes {
		return RunClaims{}, refuse(CodeDecisionInvalid, "the Space decision envelope is not valid")
	}
	parts := strings.Split(token, ".")
	if len(parts) != 4 || parts[0] != RunEnvelopeVersion {
		return RunClaims{}, refuse(CodeDecisionInvalid, "the Space decision envelope is not valid")
	}
	keyID, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil || len(keyID) == 0 {
		return RunClaims{}, refuse(CodeDecisionInvalid, "the Space decision key is not trusted")
	}
	key, ok := keys[string(keyID)]
	if !ok || len(key) != ed25519.PublicKeySize {
		return RunClaims{}, refuse(CodeDecisionInvalid, "the Space decision key is not trusted")
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[3])
	if err != nil || len(signature) != ed25519.SignatureSize {
		return RunClaims{}, refuse(CodeDecisionInvalid, "the Space decision signature is not valid")
	}
	if !ed25519.Verify(key, []byte(strings.Join(parts[:3], ".")), signature) {
		return RunClaims{}, refuse(CodeDecisionInvalid, "the Space decision signature is not valid")
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil || len(payload) == 0 {
		return RunClaims{}, refuse(CodeDecisionInvalid, "the Space decision payload is not valid")
	}
	decoder := json.NewDecoder(bytes.NewReader(payload))
	decoder.DisallowUnknownFields()
	var claims RunClaims
	if err := decoder.Decode(&claims); err != nil || decoder.Decode(&struct{}{}) != io.EOF {
		return RunClaims{}, refuse(CodeDecisionInvalid, "the Space decision payload is not valid")
	}
	if err := claims.validate(now); err != nil {
		return RunClaims{}, err
	}
	return claims, nil
}

// validate mirrors RunActionDecision.Validate and its window check. Every reference is set, the revisions are positive, and
// the decision is inside its life and was not issued in the future.
func (c RunClaims) validate(now time.Time) error {
	for _, value := range []string{
		c.DecisionRef, c.RunID, c.ThreadID, c.OrgID, c.SpaceRef, c.SubjectID, c.ServiceAudience, c.ActionID,
		c.ActionSchemaHash, c.PayloadDigest, c.IdempotencyKey, c.RecipientAudienceRef, c.RecipientAudienceHash,
		c.PrivacyPolicyRef, c.RunContextAuthorizationRef, c.Purpose, c.LawfulBasis, c.PrivacyClass,
		c.RetentionClass, c.Residency, c.DeletionScope, c.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return refuse(CodeDecisionInvalid, "the Space decision is incomplete")
		}
	}
	if len(c.Permissions) != 1 {
		return refuse(CodeDecisionInvalid, "the Space decision carries no permission")
	}
	if c.AuthorityRevision <= 0 || c.RecipientAudienceRevision <= 0 {
		return refuse(CodeDecisionInvalid, "the Space decision revisions are not positive")
	}
	if c.HistoryFloorAt != nil && c.HistoryFloorAt.IsZero() {
		return refuse(CodeDecisionInvalid, "the Space decision history floor is not valid")
	}
	if c.IssuedAt.IsZero() || c.ExpiresAt.IsZero() || !c.ExpiresAt.After(c.IssuedAt) {
		return refuse(CodeDecisionInvalid, "the Space decision window is not valid")
	}
	if !now.Before(c.ExpiresAt) {
		return refuse(CodeDecisionInvalid, "the Space decision has expired")
	}
	if c.IssuedAt.After(now.Add(clockSkew)) {
		return refuse(CodeDecisionInvalid, "the Space decision was issued in the future")
	}
	return nil
}

// CheckCanvas binds a verified agent decision to the canvas route it arrives with: the caller's org and subject, the path's
// Space, the canvas audience, the golden action, schema and permission for read or write, and no zero data retention. An
// agent decision carries no Space kind. A timestamp floor on a canvas read is a window the repository applies (canvas.Floor),
// and a floor on a write is refused by the handler, so neither is a refusal here.
func (c RunClaims) CheckCanvas(orgID, subjectID, spaceRef string, write bool) error {
	if err := c.CheckEnvelope(orgID, subjectID, spaceRef); err != nil {
		return err
	}
	if err := c.CheckAuthority(write); err != nil {
		return err
	}
	if c.ZeroDataRetention {
		return refuse(CodeDecisionMismatch, "the Space decision is issued under zero data retention")
	}
	return nil
}

// CheckEnvelope binds a verified agent decision to its addressee: the caller's org and subject, the path's Space and the canvas
// audience. A canvas route runs it before its not-found answers (D21n).
func (c RunClaims) CheckEnvelope(orgID, subjectID, spaceRef string) error {
	mismatch := func(message string) error { return refuse(CodeDecisionMismatch, message) }
	switch {
	case c.OrgID != orgID:
		return mismatch("the Space decision belongs to another organization")
	case c.SpaceRef != spaceRef:
		return mismatch("the Space decision was issued for another Space")
	case c.SubjectID != subjectID:
		return mismatch("the Space decision was issued to another user")
	case c.ServiceAudience != AudienceCanvas:
		return mismatch("the Space decision was issued for another service")
	}
	return nil
}

// CheckAuthority binds an agent decision already bound by CheckEnvelope to the golden action, schema and the one permission of a
// read or a write. It does not look at zero data retention, which the route answers last.
func (c RunClaims) CheckAuthority(write bool) error {
	action, schema, permission := ActionRunCanvasRead, SchemaRunCanvasRead, PermissionCanvasRead
	if write {
		action, schema, permission = ActionRunCanvasWrite, SchemaRunCanvasWrite, PermissionCanvasWrite
	}
	mismatch := func(message string) error { return refuse(CodeDecisionMismatch, message) }
	switch {
	case c.ActionID != action || c.ActionSchemaHash != schema:
		return mismatch("the Space decision authorizes another action")
	case !slices.Equal(c.Permissions, []string{permission}):
		return mismatch("the Space decision lacks the canvas permission")
	}
	return nil
}

// RunPayloadDigest recomputes the agent payload digest the execution lane signed. It is payload_digest in mp-contracts'
// space_self_api.rs (WS18-S1), byte for byte: the domain, a NUL byte, then each of action id, run id, org id and idempotency
// key, then each argument's name and value, every field framed as a u32 big-endian byte length and its UTF-8 bytes. No JSON is
// hashed. The arguments are the ones this request carries, flattened in the row's argument_order.
func RunPayloadDigest(actionID, runID, orgID, idempotencyKey string, args []RunArgument) string {
	h := sha256.New()
	h.Write([]byte(runPayloadDigestDomain))
	h.Write([]byte{0})
	for _, field := range []string{actionID, runID, orgID, idempotencyKey} {
		writeLengthFramed(h, field)
	}
	for _, arg := range args {
		writeLengthFramed(h, arg.Name)
		writeLengthFramed(h, arg.Value)
	}
	return "sha256:" + hex.EncodeToString(h.Sum(nil))
}

// writeLengthFramed writes one field as a u32 big-endian length and its bytes.
func writeLengthFramed(h hash.Hash, field string) {
	var length [4]byte
	binary.BigEndian.PutUint32(length[:], uint32(len(field)))
	h.Write(length[:])
	h.Write([]byte(field))
}
