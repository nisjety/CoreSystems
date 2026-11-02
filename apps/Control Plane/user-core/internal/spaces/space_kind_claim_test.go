package spaces

import (
	"encoding/json"
	"testing"
	"time"
)

// WS17-S1: every evidence-derived decision carries the kind of the Space it was issued for, so
// Model Plane can stamp it on the thread and tell a personal Space from a shared room. The claim
// is additive: it rides in the signed payload only, never in a payload digest.

func TestSpaceKindClaimSet(t *testing.T) {
	now := time.Date(2026, 10, 27, 12, 0, 0, 0, time.UTC)

	// The one assembler every issuer funnels through sets the claim from the membership kind.
	for _, kind := range []Kind{KindPersonal, KindRoom, KindProject, KindCase} {
		evidence := validPersonalThreadEvidence()
		evidence.Membership.Kind = kind
		decision := newEvidenceDecision(
			evidence, "decision-1", "model-plane", "model.thread.create", "sha256:schema",
			"sha256:digest", "idem-1", "nonce-1", []string{"thread:create"}, false, now,
		)
		if decision.SpaceKind != string(kind) {
			t.Fatalf("kind %q: decision.SpaceKind = %q", kind, decision.SpaceKind)
		}
	}

	// Padding around the kind is trimmed, like every other evidence string.
	padded := validPersonalThreadEvidence()
	padded.Membership.Kind = Kind("  room  ")
	if got := newEvidenceDecision(
		padded, "decision-1", "model-plane", "model.thread.create", "sha256:schema",
		"sha256:digest", "idem-1", "nonce-1", nil, false, now,
	).SpaceKind; got != "room" {
		t.Fatalf("padded kind = %q, want room", got)
	}

	// The public issuers: personal create, shared create and append (v1 and v2 schema).
	personal, err := IssuePersonalThreadCreateDecision(validPersonalThreadEvidence(), validPersonalThreadRequest(), now)
	if err != nil {
		t.Fatalf("IssuePersonalThreadCreateDecision: %v", err)
	}
	if personal.SpaceKind != "personal" {
		t.Fatalf("personal create space_kind = %q", personal.SpaceKind)
	}

	shared := validPersonalThreadEvidence()
	shared.Membership.Kind = KindRoom
	shared.Membership.Role = "editor"
	created, err := IssueSharedThreadCreateDecision(shared, validPersonalThreadRequest(), now)
	if err != nil {
		t.Fatalf("IssueSharedThreadCreateDecision: %v", err)
	}
	if created.SpaceKind != "room" {
		t.Fatalf("shared create space_kind = %q", created.SpaceKind)
	}

	golden := loadThreadAppendGolden(t)
	evidence, request := golden.evidenceAndRequest()
	for _, postKind := range []string{"", ThreadAppendPostKindTurn, ThreadAppendPostKindPost} {
		request.PostKind = postKind
		appended, err := IssueThreadAppendDecision(evidence, request, now)
		if err != nil {
			t.Fatalf("post_kind %q: IssueThreadAppendDecision: %v", postKind, err)
		}
		if appended.SpaceKind != "room" {
			t.Fatalf("post_kind %q: append space_kind = %q", postKind, appended.SpaceKind)
		}
	}
}

func TestSpaceKindClaimIsOmittedWhenEmptyAndNamedWhenSet(t *testing.T) {
	now := time.Date(2026, 10, 27, 12, 0, 0, 0, time.UTC)
	evidence := validPersonalThreadEvidence()
	evidence.Membership.Kind = ""
	empty := newEvidenceDecision(
		evidence, "decision-1", "model-plane", "model.thread.create", "sha256:schema",
		"sha256:digest", "idem-1", "nonce-1", nil, false, now,
	)
	raw, err := json.Marshal(empty)
	if err != nil {
		t.Fatalf("marshal empty: %v", err)
	}
	var asMap map[string]any
	if err := json.Unmarshal(raw, &asMap); err != nil {
		t.Fatalf("unmarshal empty: %v", err)
	}
	if _, present := asMap["space_kind"]; present {
		t.Fatalf("an empty kind must be omitted from the signed payload: %s", raw)
	}

	evidence.Membership.Kind = KindRoom
	set := newEvidenceDecision(
		evidence, "decision-1", "model-plane", "model.thread.create", "sha256:schema",
		"sha256:digest", "idem-1", "nonce-1", nil, false, now,
	)
	raw, err = json.Marshal(set)
	if err != nil {
		t.Fatalf("marshal set: %v", err)
	}
	asMap = map[string]any{}
	if err := json.Unmarshal(raw, &asMap); err != nil {
		t.Fatalf("unmarshal set: %v", err)
	}
	if asMap["space_kind"] != "room" {
		t.Fatalf("space_kind claim = %v, want room", asMap["space_kind"])
	}
}

// The claim is not an input to any payload digest: the same effect issued for a personal Space
// and for a room differs only in the fields the digest already binds, never in space_kind.
func TestSpaceKindClaimDoesNotChangeTheGoldenDigest(t *testing.T) {
	golden := loadThreadAppendGolden(t)
	evidence, request := golden.evidenceAndRequest()
	now := time.Date(2026, 10, 27, 12, 0, 0, 0, time.UTC)

	request.PostKind = ThreadAppendPostKindTurn
	before := threadAppendPayloadDigest(evidence, request)
	if before != golden.Expected.V1Digest {
		t.Fatalf("v1 digest = %s, want the pinned golden %s", before, golden.Expected.V1Digest)
	}
	decision, err := IssueThreadAppendDecision(evidence, request, now)
	if err != nil {
		t.Fatalf("IssueThreadAppendDecision: %v", err)
	}
	if decision.SpaceKind != "room" || decision.PayloadDigest != golden.Expected.V1Digest {
		t.Fatalf("decision kind %q digest %q: the kind claim must not move the digest", decision.SpaceKind, decision.PayloadDigest)
	}
}
