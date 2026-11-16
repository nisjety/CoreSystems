package spacedecision

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"hash"
)

// The canvas operations, named as the digest names them. They are Control's Op set. Read carries no canvas id, expected
// version, restore source or content digest.
const (
	OpRead    = "read"
	OpCreate  = "create"
	OpSave    = "save"
	OpRestore = "restore"
)

// canvasDigestHeader opens the canvas payload digest. It is Control's canvasDigestHeader, byte for byte.
const canvasDigestHeader = "data.space_canvas\x00v1\x00"

// CanvasOp is one canvas request as the handler derives it. Empty strings stand for absent fields, as they do on the
// Control side, where a null on the wire becomes an empty value.
type CanvasOp struct {
	Op                   string
	Schema               string
	CanvasID             string
	ExpectedVersionID    string
	RestoreFromVersionID string
	ContentDigest        string
	IdempotencyKey       string
}

// CanvasPayloadDigest recomputes the payload digest Control signed for a canvas decision. It is canvasPayloadDigest in
// user-core's canvas_decision.go. The identity and authority fields come from the verified claims. The operation fields
// come from op. The encoding is the header, then for each field its name, a NUL byte, an 8-byte big-endian length and
// the value, then for each revision its name, a NUL byte and an 8-byte big-endian value.
func CanvasPayloadDigest(c Claims, op CanvasOp) string {
	h := sha256.New()
	h.Write([]byte(canvasDigestHeader))
	for _, field := range []struct{ name, value string }{
		{"org_id", c.OrgID},
		{"user_id", c.SubjectID},
		{"space_id", c.SpaceRef},
		{"space_decision_ref", c.DecisionRef},
		{"recipient_audience_ref", c.RecipientAudienceRef},
		{"recipient_audience_hash", c.RecipientAudienceHash},
		{"privacy_policy_ref", c.PrivacyPolicyRef},
		{"resource_authorization_ref", c.ResourceAuthorizationRef},
		{"action_schema_hash", op.Schema},
		{"idempotency_key", op.IdempotencyKey},
		{"op", op.Op},
		{"canvas_id", op.CanvasID},
		{"expected_version_id", op.ExpectedVersionID},
		{"restore_from_version_id", op.RestoreFromVersionID},
		{"content_digest", op.ContentDigest},
	} {
		h.Write([]byte(field.name))
		h.Write([]byte{0})
		var length [8]byte
		binary.BigEndian.PutUint64(length[:], uint64(len(field.value)))
		h.Write(length[:])
		h.Write([]byte(field.value))
	}
	writeRevision(h, "authority_revision", c.AuthorityRevision)
	writeRevision(h, "recipient_audience_revision", c.RecipientAudienceRevision)
	return "sha256:" + hex.EncodeToString(h.Sum(nil))
}

// writeRevision binds one named integer revision the way Control's writeDigestRevisions does.
func writeRevision(h hash.Hash, name string, value int64) {
	h.Write([]byte(name))
	h.Write([]byte{0})
	var encoded [8]byte
	binary.BigEndian.PutUint64(encoded[:], uint64(value))
	h.Write(encoded[:])
}
