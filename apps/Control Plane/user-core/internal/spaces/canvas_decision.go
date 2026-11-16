package spaces

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"strings"
	"time"
	"unicode/utf8"
)

// WS19-S2: the room canvas decision family (data.canvas.read, data.canvas.write).
//
// A canvas is one Markdown document per Space, stored in Data Plane (wiki-store).
// Control decides who may read or write it and signs a short-lived decision that
// wiki-store verifies itself (WS19-S3). The decision carries the operation, the
// expected head and a SHA-256 commitment to the content, never the content.
// canvas_entitled is deny by default and no preset enables it. Under Space ZDR
// Control refuses every canvas decision, read and write.
const (
	canvasAudience = "data-plane-canvas"

	canvasReadAction      = "data.canvas.read"
	canvasWriteAction     = "data.canvas.write"
	canvasReadSchema      = "sha256:space-canvas-read-v1"
	canvasWriteSchema     = "sha256:space-canvas-write-v1"
	canvasReadPermission  = "canvas:read"
	canvasWritePermission = "canvas:write"

	// canvasDigestHeader opens the payload digest. wiki-store recomputes the same
	// bytes, so the header is part of the wire contract.
	canvasDigestHeader = "data.space_canvas\x00v1\x00"

	// canvasMaxIDBytes bounds canvas and version ids and canvasMaxIdempotencyKeyBytes
	// the key. They match wiki-store's MaxVersionIDBytes and MaxIdempotencyKeyBytes,
	// so every value Control signs is one Data Plane accepts.
	canvasMaxIDBytes             = 128
	canvasMaxIdempotencyKeyBytes = 256
)

// The canvas operations the issuer accepts. Anything else is refused before any
// authority is read.
const (
	CanvasOpRead    = "read"
	CanvasOpCreate  = "create"
	CanvasOpSave    = "save"
	CanvasOpRestore = "restore"
)

// CanvasDecisionRequest carries only the operation-bound fields the verified
// gateway path supplies. It names no actor, organisation, role, audience, policy,
// revision or content. ExpectedVersionID and RestoreFromVersionID are "" when
// absent (null on the wire). ContentDigest is the "sha256:" commitment to the
// content bytes, computed by the gateway and recomputed by wiki-store; the bytes
// never reach Control.
type CanvasDecisionRequest struct {
	DecisionRef          string
	Op                   string
	CanvasID             string
	ExpectedVersionID    string
	RestoreFromVersionID string
	ContentDigest        string
	IdempotencyKey       string
	Nonce                string
}

// IsWrite reports whether the operation changes the canvas.
func (r CanvasDecisionRequest) IsWrite() bool {
	return r.Op != CanvasOpRead
}

// Validate checks the request's shape before any authority is read: the operation,
// the fields it may carry, and each identifier's bound. An empty optional
// identifier is accepted. Surrounding whitespace is refused rather than trimmed,
// so the digest binds exactly the bytes the caller sent.
func (r CanvasDecisionRequest) Validate() error {
	for label, value := range map[string]string{
		"decision_ref":    r.DecisionRef,
		"idempotency_key": r.IdempotencyKey,
		"nonce":           r.Nonce,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("canvas decision %s is required", label)
		}
	}
	if !canvasTokenOK(r.IdempotencyKey, canvasMaxIdempotencyKeyBytes) {
		return fmt.Errorf("canvas decision idempotency_key is invalid")
	}
	for label, value := range map[string]string{
		"canvas_id":               r.CanvasID,
		"expected_version_id":     r.ExpectedVersionID,
		"restore_from_version_id": r.RestoreFromVersionID,
	} {
		if !canvasTokenOK(value, canvasMaxIDBytes) {
			return fmt.Errorf("canvas decision %s is invalid", label)
		}
	}
	if r.ContentDigest != "" && !validCanvasContentDigest(r.ContentDigest) {
		return fmt.Errorf("canvas decision content_digest is invalid")
	}
	switch r.Op {
	case CanvasOpRead:
		if r.ContentDigest != "" || r.ExpectedVersionID != "" || r.RestoreFromVersionID != "" {
			return fmt.Errorf("canvas read carries no content, expected version or restore source")
		}
	case CanvasOpCreate:
		if r.CanvasID != "" || r.ExpectedVersionID != "" || r.RestoreFromVersionID != "" {
			return fmt.Errorf("canvas create carries no canvas id, expected version or restore source")
		}
		if r.ContentDigest == "" {
			return fmt.Errorf("canvas create requires content_digest")
		}
	case CanvasOpSave:
		if r.RestoreFromVersionID != "" {
			return fmt.Errorf("canvas save carries no restore source")
		}
		if r.ContentDigest == "" {
			return fmt.Errorf("canvas save requires content_digest")
		}
	case CanvasOpRestore:
		if r.ContentDigest != "" {
			return fmt.Errorf("canvas restore carries no content_digest")
		}
		if r.RestoreFromVersionID == "" {
			return fmt.Errorf("canvas restore requires restore_from_version_id")
		}
	default:
		return fmt.Errorf("canvas decision op is not recognized")
	}
	return nil
}

// canvasTokenOK accepts an empty value or a bounded token: at most limit bytes,
// valid UTF-8, no NUL byte and no surrounding whitespace.
func canvasTokenOK(value string, limit int) bool {
	return len(value) <= limit && utf8.ValidString(value) &&
		!strings.ContainsRune(value, 0) && strings.TrimSpace(value) == value
}

// validCanvasContentDigest accepts "sha256:" followed by 64 lowercase hex digits,
// the form wiki-store stores and recomputes (ContentDigest in its canvas package).
func validCanvasContentDigest(value string) bool {
	const prefix = "sha256:"
	if len(value) != len(prefix)+sha256.Size*2 || !strings.HasPrefix(value, prefix) {
		return false
	}
	for i := len(prefix); i < len(value); i++ {
		c := value[i]
		if (c < '0' || c > '9') && (c < 'a' || c > 'f') {
			return false
		}
	}
	return true
}

// ValidateForCanvas authorizes a canvas read (write false) or a canvas write (write
// true) by the current member the evidence describes.
//
// The kinds are personal, room, project and case. A direct message or a group
// direct message has no canvas (D-8). A shared Space needs a current recipient
// audience, which the resolver's join has already proved for the caller; a
// personal Space needs its one recipient. The role floor is viewer to read and
// editor to write. canvas_entitled must be on, and Zero Data Retention refuses
// every canvas decision, read and write (D-5).
func (e PersonalThreadDecisionEvidence) ValidateForCanvas(write bool) error {
	if err := e.Membership.Validate(); err != nil {
		return err
	}
	switch e.Membership.Kind {
	case KindPersonal:
		if err := e.validatePersonalAuthority(); err != nil {
			return err
		}
	case KindRoom, KindProject, KindCase:
		if strings.TrimSpace(e.RecipientAudienceRef) == "" || strings.TrimSpace(e.RecipientAudienceHash) == "" {
			return authorityIncompleteDenial("shared Space recipient audience is required")
		}
		if strings.TrimSpace(e.ResourceAuthorizationRef) == "" {
			return authorityIncompleteDenial("shared Space canvas authority is incomplete")
		}
		if err := e.Privacy.Validate(); err != nil {
			return err
		}
	default:
		return kindUnsupportedDenial(string(e.Membership.Kind), "a canvas is available in personal, room, project and case Spaces only")
	}
	floor := []string{"viewer", "editor", "manager", "owner"}
	if write {
		floor = []string{"editor", "manager", "owner"}
	}
	if !matchesOneOf(e.Membership.Role, floor...) {
		return roleBelowFloorDenial(fmt.Sprintf("space role %q cannot use the canvas", e.Membership.Role), floor...)
	}
	if !e.CanvasEntitled {
		return entitlementOffDenial("canvas", "canvas authority is not active for this organization")
	}
	if e.Privacy.ZeroDataRetention {
		return zdrForbidsDenial("canvas", "the canvas is not available under zero data retention")
	}
	return nil
}

// canvasPayloadDigest binds the exact canvas effect Data Plane verifies. The
// header is followed by length-prefixed fields (the same encoding as
// threadAppendPayloadDigest), then the two revisions as 8-byte big-endian values.
// The operation, canvas, expected head, restore source and content commitment are
// all bound, so a decision cannot be replayed for other text or another head.
// wiki-store recomputes this formula; the golden vector in testdata pins it.
func canvasPayloadDigest(evidence PersonalThreadDecisionEvidence, request CanvasDecisionRequest, schema string) string {
	hash := sha256.New()
	hash.Write([]byte(canvasDigestHeader))
	for _, field := range []struct{ name, value string }{
		{"org_id", evidence.Membership.OrgID},
		{"user_id", evidence.Membership.SubjectID},
		{"space_id", evidence.Membership.SpaceRef},
		{"space_decision_ref", request.DecisionRef},
		{"recipient_audience_ref", evidence.RecipientAudienceRef},
		{"recipient_audience_hash", evidence.RecipientAudienceHash},
		{"privacy_policy_ref", evidence.Privacy.PolicyRef},
		{"resource_authorization_ref", evidence.ResourceAuthorizationRef},
		{"action_schema_hash", schema},
		{"idempotency_key", request.IdempotencyKey},
		{"op", request.Op},
		{"canvas_id", request.CanvasID},
		{"expected_version_id", request.ExpectedVersionID},
		{"restore_from_version_id", request.RestoreFromVersionID},
		{"content_digest", request.ContentDigest},
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
		digestRevision{"recipient_audience_revision", evidence.Membership.Revisions.RecipientAudience},
	)
	return "sha256:" + hex.EncodeToString(hash.Sum(nil))
}

// IssueCanvasDecision authorizes one short-lived canvas read or write. The action,
// schema and permission follow the operation, so a read token cannot write and a
// write token cannot be presented as a read. The decision carries no content and
// no head: wiki-store checks the expected head in its own transaction and
// recomputes the digest from the bytes it received.
func IssueCanvasDecision(evidence PersonalThreadDecisionEvidence, request CanvasDecisionRequest, now time.Time) (Decision, error) {
	if err := request.Validate(); err != nil {
		return Decision{}, err
	}
	if now.IsZero() {
		return Decision{}, fmt.Errorf("canvas decision issuance time is required")
	}
	write := request.IsWrite()
	if err := evidence.ValidateForCanvas(write); err != nil {
		return Decision{}, err
	}
	action, schema, permission := canvasReadAction, canvasReadSchema, canvasReadPermission
	if write {
		action, schema, permission = canvasWriteAction, canvasWriteSchema, canvasWritePermission
	}
	decision := newEvidenceDecision(
		evidence, request.DecisionRef, canvasAudience, action, schema,
		canvasPayloadDigest(evidence, request, schema), request.IdempotencyKey, request.Nonce,
		[]string{permission}, evidence.Privacy.ZeroDataRetention, now,
	)
	if evidence.HistoryFloorRevision > 0 {
		decision.HistoryFloorRevision = evidence.HistoryFloorRevision
	}
	return decision, nil
}
