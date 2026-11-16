package spaces

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"strings"
	"time"
)

// W3-DEC-KIND (decision D17): signed evidence that a Space is the caller's personal Space. Session Core restores a
// withheld legacy note only when every thread it links to is personal or sits in no Space. A personal-Space thread
// that never recorded its kind is refused as unknown, so its note stays withheld. This claim lets Session Core accept
// exactly that thread, and only for the Space the claim names.
//
// The claim is signed with the Control Space decision key and the same Ed25519 envelope as every other Space
// decision (version.key-id.payload.signature). Its version tag is distinct from the decision version, because the tag
// is part of the signed bytes: a decision never verifies as evidence, or evidence as a decision, by construction.
const (
	SpaceKindEvidenceVersion = "space-kind-v1"
	// SpaceKindEvidenceTTL bounds a claim's life. The gateway asks for one immediately before the restore it serves.
	SpaceKindEvidenceTTL = 2 * time.Minute
)

// SpaceKindEvidence is the claim. It carries Space facts only: the organization, the Space, the subject the answer
// is for, the kind Control holds for that Space, and the issue and expiry times. It never carries content.
type SpaceKindEvidence struct {
	OrgID     string    `json:"org_id"`
	SpaceRef  string    `json:"space_ref"`
	SubjectID string    `json:"subject_id"`
	Kind      string    `json:"kind"`
	IssuedAt  time.Time `json:"issued_at"`
	ExpiresAt time.Time `json:"expires_at"`
}

// IssueSpaceKindEvidence states the kind of a Space for the caller. The membership must come from Control's readable
// resolver for that caller, so the organization, subject, Space and kind are Control's facts, never the caller's
// assertion. A caller with no readable membership never reaches this function.
func IssueSpaceKindEvidence(membership *CurrentMembership, now time.Time) (SpaceKindEvidence, error) {
	if membership == nil {
		return SpaceKindEvidence{}, fmt.Errorf("space kind evidence requires a current membership")
	}
	if now.IsZero() {
		return SpaceKindEvidence{}, fmt.Errorf("space kind evidence issuance time is required")
	}
	kind := strings.TrimSpace(string(membership.Kind))
	evidence := SpaceKindEvidence{
		OrgID:     membership.OrgID,
		SpaceRef:  membership.SpaceRef,
		SubjectID: membership.SubjectID,
		Kind:      kind,
		IssuedAt:  now.UTC(),
		ExpiresAt: now.UTC().Add(SpaceKindEvidenceTTL),
	}
	if err := evidence.validate(); err != nil {
		return SpaceKindEvidence{}, err
	}
	return evidence, nil
}

func (e SpaceKindEvidence) validate() error {
	for label, value := range map[string]string{
		"org_id":     e.OrgID,
		"space_ref":  e.SpaceRef,
		"subject_id": e.SubjectID,
		"kind":       e.Kind,
	} {
		if strings.TrimSpace(value) == "" {
			return fmt.Errorf("space kind evidence %s is required", label)
		}
	}
	if e.IssuedAt.IsZero() || e.ExpiresAt.IsZero() || !e.ExpiresAt.After(e.IssuedAt) {
		return fmt.Errorf("space kind evidence expiry must be after issuance")
	}
	return nil
}

// SignSpaceKindEvidence issues the key-identified envelope. The private key stays in Control; Session Core verifies
// the claim with the matching public key from its deployment-managed key set.
func SignSpaceKindEvidence(key SigningKey, evidence SpaceKindEvidence) (string, error) {
	if strings.TrimSpace(key.ID) == "" || len(key.PrivateKey) != ed25519.PrivateKeySize {
		return "", fmt.Errorf("valid Space decision signing key is required")
	}
	if err := evidence.validate(); err != nil {
		return "", err
	}
	payload, err := json.Marshal(evidence)
	if err != nil {
		return "", fmt.Errorf("marshal space kind evidence: %w", err)
	}
	encodedKeyID := base64.RawURLEncoding.EncodeToString([]byte(key.ID))
	encoded := base64.RawURLEncoding.EncodeToString(payload)
	signed := SpaceKindEvidenceVersion + "." + encodedKeyID + "." + encoded
	signature := ed25519.Sign(key.PrivateKey, []byte(signed))
	return signed + "." + base64.RawURLEncoding.EncodeToString(signature), nil
}
