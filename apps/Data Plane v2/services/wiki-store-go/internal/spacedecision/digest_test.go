package spacedecision

import (
	"encoding/json"
	"os"
	"strings"
	"testing"
)

// canvasGoldenPath is Control's shared vector for the room canvas payload digest (WS19-S2). It is read-only here. The path
// runs from this package directory up to apps/, then into the Control user-core testdata.
const canvasGoldenPath = "../../../../../Control Plane/user-core/internal/spaces/testdata/space-canvas-decision.golden.json"

type canvasGolden struct {
	Header string `json:"header"`
	Input  struct {
		OrgID                     string `json:"org_id"`
		UserID                    string `json:"user_id"`
		SpaceID                   string `json:"space_id"`
		SpaceDecisionRef          string `json:"space_decision_ref"`
		RecipientAudienceRef      string `json:"recipient_audience_ref"`
		RecipientAudienceHash     string `json:"recipient_audience_hash"`
		PrivacyPolicyRef          string `json:"privacy_policy_ref"`
		ResourceAuthorizationRef  string `json:"resource_authorization_ref"`
		AuthorityRevision         int64  `json:"authority_revision"`
		RecipientAudienceRevision int64  `json:"recipient_audience_revision"`
	} `json:"input"`
	Vectors []canvasGoldenVector `json:"vectors"`
}

type canvasGoldenVector struct {
	Name                 string `json:"name"`
	Op                   string `json:"op"`
	ActionSchemaHash     string `json:"action_schema_hash"`
	IdempotencyKey       string `json:"idempotency_key"`
	CanvasID             string `json:"canvas_id"`
	ExpectedVersionID    string `json:"expected_version_id"`
	RestoreFromVersionID string `json:"restore_from_version_id"`
	ContentDigest        string `json:"content_digest"`
	PayloadDigest        string `json:"payload_digest"`
}

// TestCanvasPayloadDigestReproducesControlGoldenVectors recomputes the four canvas payload digests (read, create, save,
// restore) from the golden input. Any drift between this port and Control's canvasPayloadDigest fails here.
func TestCanvasPayloadDigestReproducesControlGoldenVectors(t *testing.T) {
	raw, err := os.ReadFile(canvasGoldenPath)
	if err != nil {
		t.Fatalf("read the Control canvas golden: %v", err)
	}
	var golden canvasGolden
	if err := json.Unmarshal(raw, &golden); err != nil {
		t.Fatalf("decode the Control canvas golden: %v", err)
	}
	// The golden spells the header with literal \x00 escapes. It must name the same bytes as canvasDigestHeader.
	if got := strings.ReplaceAll(golden.Header, `\x00`, "\x00"); got != canvasDigestHeader {
		t.Fatalf("golden header %q does not name canvasDigestHeader %q", got, canvasDigestHeader)
	}
	if len(golden.Vectors) != 4 {
		t.Fatalf("golden has %d vectors, want read, create, save and restore", len(golden.Vectors))
	}

	claims := Claims{
		OrgID:                     golden.Input.OrgID,
		SubjectID:                 golden.Input.UserID,
		SpaceRef:                  golden.Input.SpaceID,
		DecisionRef:               golden.Input.SpaceDecisionRef,
		RecipientAudienceRef:      golden.Input.RecipientAudienceRef,
		RecipientAudienceHash:     golden.Input.RecipientAudienceHash,
		PrivacyPolicyRef:          golden.Input.PrivacyPolicyRef,
		ResourceAuthorizationRef:  golden.Input.ResourceAuthorizationRef,
		AuthorityRevision:         golden.Input.AuthorityRevision,
		RecipientAudienceRevision: golden.Input.RecipientAudienceRevision,
	}
	for _, vector := range golden.Vectors {
		t.Run(vector.Name, func(t *testing.T) {
			op := CanvasOp{
				Op:                   vector.Op,
				Schema:               vector.ActionSchemaHash,
				CanvasID:             vector.CanvasID,
				ExpectedVersionID:    vector.ExpectedVersionID,
				RestoreFromVersionID: vector.RestoreFromVersionID,
				ContentDigest:        vector.ContentDigest,
				IdempotencyKey:       vector.IdempotencyKey,
			}
			if got := CanvasPayloadDigest(claims, op); got != vector.PayloadDigest {
				t.Fatalf("%s payload digest = %s, golden %s", vector.Name, got, vector.PayloadDigest)
			}
		})
	}
}
