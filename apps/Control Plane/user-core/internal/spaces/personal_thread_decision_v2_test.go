package spaces

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"
)

// WS01-S2: thread-append schema v2 binds post_kind. The golden
// testdata/thread-append-v2.golden.json is shared with Session Core (sha-pinned
// copy in its grpc.rs tests), so a one-sided encoding change fails on both
// sides.

type threadAppendGolden struct {
	Input struct {
		OrgID                    string `json:"org_id"`
		UserID                   string `json:"user_id"`
		ThreadID                 string `json:"thread_id"`
		SpaceID                  string `json:"space_id"`
		SpaceDecisionRef         string `json:"space_decision_ref"`
		RecipientAudienceRef     string `json:"recipient_audience_ref"`
		RecipientAudienceHash    string `json:"recipient_audience_hash"`
		PrivacyPolicyRef         string `json:"privacy_policy_ref"`
		ResourceAuthorizationRef string `json:"resource_authorization_ref"`
		Content                  string `json:"content"`
		IdempotencyKey           string `json:"idempotency_key"`
		AuthorityRevision        int64  `json:"authority_revision"`
		RecipientAudienceRev     int64  `json:"recipient_audience_revision"`
	} `json:"input"`
	Expected struct {
		V1Schema     string `json:"v1_schema"`
		V1Digest     string `json:"v1_digest"`
		V2Schema     string `json:"v2_schema"`
		V2PostDigest string `json:"v2_post_digest"`
	} `json:"expected"`
}

func loadThreadAppendGolden(t *testing.T) threadAppendGolden {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join("testdata", "thread-append-v2.golden.json"))
	if err != nil {
		t.Fatalf("read golden: %v", err)
	}
	var golden threadAppendGolden
	if err := json.Unmarshal(raw, &golden); err != nil {
		t.Fatalf("parse golden: %v", err)
	}
	return golden
}

func (g threadAppendGolden) evidenceAndRequest() (PersonalThreadDecisionEvidence, ThreadAppendDecisionRequest) {
	evidence := validPersonalThreadEvidence()
	evidence.Membership.Kind = KindRoom
	evidence.Membership.Role = "editor"
	evidence.Membership.OrgID = g.Input.OrgID
	evidence.Membership.SubjectID = g.Input.UserID
	evidence.Membership.SpaceRef = g.Input.SpaceID
	evidence.RecipientAudienceRef = g.Input.RecipientAudienceRef
	evidence.RecipientAudienceHash = g.Input.RecipientAudienceHash
	evidence.Privacy.PolicyRef = g.Input.PrivacyPolicyRef
	evidence.ResourceAuthorizationRef = g.Input.ResourceAuthorizationRef
	evidence.Membership.Revisions.Authority = g.Input.AuthorityRevision
	evidence.Membership.Revisions.RecipientAudience = g.Input.RecipientAudienceRev
	request := ThreadAppendDecisionRequest{
		DecisionRef: g.Input.SpaceDecisionRef, ThreadID: g.Input.ThreadID,
		ContentDigest:  "sha256:" + hex.EncodeToString(sha256Bytes(g.Input.Content)),
		IdempotencyKey: g.Input.IdempotencyKey, Nonce: "nonce-1",
	}
	return evidence, request
}

func sha256Bytes(value string) []byte {
	sum := sha256.Sum256([]byte(value))
	return sum[:]
}

// referenceThreadAppendDigest is an independent re-statement of the wire
// encoding (header, length-prefixed fields, then the revisions) so the pinned
// hex is not merely the production function agreeing with itself.
func referenceThreadAppendDigest(header string, schema string, g threadAppendGolden, contentDigest string, postKind string) string {
	hash := sha256.New()
	hash.Write([]byte(header))
	write := func(name, value string) {
		hash.Write([]byte(name))
		hash.Write([]byte{0})
		var length [8]byte
		binary.BigEndian.PutUint64(length[:], uint64(len(value)))
		hash.Write(length[:])
		hash.Write([]byte(value))
	}
	write("org_id", g.Input.OrgID)
	write("user_id", g.Input.UserID)
	write("thread_id", g.Input.ThreadID)
	write("space_id", g.Input.SpaceID)
	write("space_decision_ref", g.Input.SpaceDecisionRef)
	write("recipient_audience_ref", g.Input.RecipientAudienceRef)
	write("recipient_audience_hash", g.Input.RecipientAudienceHash)
	write("privacy_policy_ref", g.Input.PrivacyPolicyRef)
	write("resource_authorization_ref", g.Input.ResourceAuthorizationRef)
	write("content_digest", contentDigest)
	write("action_schema_hash", schema)
	write("idempotency_key", g.Input.IdempotencyKey)
	if postKind != "" {
		write("post_kind", postKind)
	}
	hash.Write([]byte("authority_revision\x00"))
	var rev [8]byte
	binary.BigEndian.PutUint64(rev[:], uint64(g.Input.AuthorityRevision))
	hash.Write(rev[:])
	hash.Write([]byte("recipient_audience_revision\x00"))
	binary.BigEndian.PutUint64(rev[:], uint64(g.Input.RecipientAudienceRev))
	hash.Write(rev[:])
	return "sha256:" + hex.EncodeToString(hash.Sum(nil))
}

func TestThreadAppendV2DigestGolden(t *testing.T) {
	golden := loadThreadAppendGolden(t)
	evidence, request := golden.evidenceAndRequest()

	request.PostKind = ThreadAppendPostKindPost
	if got := threadAppendPayloadDigest(evidence, request); got != golden.Expected.V2PostDigest {
		t.Fatalf("v2 post digest drifted from the shared golden:\n got %s\nwant %s", got, golden.Expected.V2PostDigest)
	}
	if got := referenceThreadAppendDigest("model.thread.append\x00v2\x00", golden.Expected.V2Schema, golden, request.ContentDigest, "post"); got != golden.Expected.V2PostDigest {
		t.Fatalf("golden v2 hex does not match the reference encoding: %s", got)
	}
	if golden.Expected.V2Schema != threadAppendSchemaV2 {
		t.Fatalf("golden v2 schema %q != Control constant %q", golden.Expected.V2Schema, threadAppendSchemaV2)
	}

	// The Rust copy, when this checkout has it, must be byte for byte the same
	// (line endings normalized: autocrlf rewrites checkouts on Windows).
	rustCopy := filepath.Join("..", "..", "..", "..", "Model Plane", "rust", "services", "session-core", "tests", "fixtures", "thread-append-v2.golden.json")
	if rust, err := os.ReadFile(rustCopy); err == nil {
		ours, _ := os.ReadFile(filepath.Join("testdata", "thread-append-v2.golden.json"))
		if !bytes.Equal(normalizeEOL(rust), normalizeEOL(ours)) {
			t.Fatal("Session Core's copy of thread-append-v2.golden.json differs from Control's")
		}
	}
}

func normalizeEOL(raw []byte) []byte {
	return bytes.ReplaceAll(raw, []byte("\r\n"), []byte("\n"))
}

func TestThreadAppendV1DigestUnchanged(t *testing.T) {
	golden := loadThreadAppendGolden(t)
	evidence, request := golden.evidenceAndRequest()

	for _, kind := range []string{"", ThreadAppendPostKindTurn} {
		request.PostKind = kind
		if got := threadAppendPayloadDigest(evidence, request); got != golden.Expected.V1Digest {
			t.Fatalf("post_kind %q: v1 digest changed:\n got %s\nwant %s", kind, got, golden.Expected.V1Digest)
		}
		decision, err := IssueThreadAppendDecision(evidence, request, time.Date(2026, 10, 1, 12, 0, 0, 0, time.UTC))
		if err != nil {
			t.Fatalf("post_kind %q: IssueThreadAppendDecision: %v", kind, err)
		}
		if decision.ActionSchemaHash != threadAppendSchema || decision.ActionSchemaHash != golden.Expected.V1Schema {
			t.Fatalf("post_kind %q issued schema %q, want v1", kind, decision.ActionSchemaHash)
		}
		if decision.PayloadDigest != golden.Expected.V1Digest {
			t.Fatalf("post_kind %q: decision digest %q != v1 golden", kind, decision.PayloadDigest)
		}
	}
	if got := referenceThreadAppendDigest("model.thread.append\x00v1\x00", golden.Expected.V1Schema, golden, request.ContentDigest, ""); got != golden.Expected.V1Digest {
		t.Fatalf("golden v1 hex does not match the reference encoding: %s", got)
	}
}

func TestThreadAppendPostDecisionBindsMemberSubjectAndKind(t *testing.T) {
	golden := loadThreadAppendGolden(t)
	evidence, request := golden.evidenceAndRequest()
	now := time.Date(2026, 10, 1, 12, 0, 0, 0, time.UTC)

	request.PostKind = ThreadAppendPostKindPost
	post, err := IssueThreadAppendDecision(evidence, request, now)
	if err != nil {
		t.Fatalf("post decision: %v", err)
	}
	if post.ActionSchemaHash != "sha256:thread-append-v2" {
		t.Fatalf("post decision schema = %q, want thread-append-v2", post.ActionSchemaHash)
	}
	if post.SubjectID != evidence.Membership.SubjectID || post.SubjectID != "user-2" {
		t.Fatalf("post decision subject = %q, want the caller", post.SubjectID)
	}
	if post.ActionID != threadAppendAction || len(post.Permissions) != 1 || post.Permissions[0] != "thread:append" {
		t.Fatalf("post decision action/permissions drifted: %+v", post)
	}
	if post.SpaceKind != string(KindRoom) {
		t.Fatalf("post decision space_kind = %q", post.SpaceKind)
	}

	request.PostKind = ThreadAppendPostKindTurn
	turn, err := IssueThreadAppendDecision(evidence, request, now)
	if err != nil {
		t.Fatalf("turn decision: %v", err)
	}
	if post.PayloadDigest == turn.PayloadDigest {
		t.Fatal("payload digest did not change with post_kind")
	}

	// A different caller cannot reuse the post digest.
	other := evidence
	other.Membership.SubjectID = "user-3"
	request.PostKind = ThreadAppendPostKindPost
	otherPost, err := IssueThreadAppendDecision(other, request, now)
	if err != nil {
		t.Fatalf("other member post: %v", err)
	}
	if otherPost.PayloadDigest == post.PayloadDigest || otherPost.SubjectID != "user-3" {
		t.Fatal("post decision is not bound to the member subject")
	}

	// post_kind is a closed vocabulary.
	request.PostKind = "x"
	if _, err := IssueThreadAppendDecision(evidence, request, now); err == nil {
		t.Fatal("unknown post_kind was issued")
	}
	for _, kind := range []string{"", "turn", "post", " post "} {
		if !ValidThreadAppendPostKind(kind) {
			t.Errorf("post_kind %q should be valid", kind)
		}
	}
	for _, kind := range []string{"x", "Post", "TURN", "post,turn"} {
		if ValidThreadAppendPostKind(kind) {
			t.Errorf("post_kind %q should be refused", kind)
		}
	}
}

func TestThreadAppendPostRefusedForViewerAndNonRecipient(t *testing.T) {
	golden := loadThreadAppendGolden(t)
	now := time.Date(2026, 10, 1, 12, 0, 0, 0, time.UTC)

	_, request := golden.evidenceAndRequest()
	request.PostKind = ThreadAppendPostKindPost

	viewer, _ := golden.evidenceAndRequest()
	viewer.Membership.Role = "viewer"
	_, err := IssueThreadAppendDecision(viewer, request, now)
	if err == nil {
		t.Fatal("a viewer was issued a post decision")
	}
	if denial, ok := AsDenial(err); !ok || denial.Code != DenialRoleBelowFloor {
		t.Fatalf("viewer refusal = %v, want a role_below_floor denial", err)
	}

	// A caller outside the Space's current recipient audience reaches Control
	// with no resolved audience; the decision must not issue.
	nonRecipient, _ := golden.evidenceAndRequest()
	nonRecipient.RecipientAudienceRef = ""
	nonRecipient.RecipientAudienceHash = ""
	if _, err := IssueThreadAppendDecision(nonRecipient, request, now); err == nil {
		t.Fatal("a non-recipient was issued a post decision")
	}

	// Entitlement off refuses as well.
	unentitled, _ := golden.evidenceAndRequest()
	unentitled.ThreadCreateEntitled = false
	if _, err := IssueThreadAppendDecision(unentitled, request, now); err == nil {
		t.Fatal("an unentitled org was issued a post decision")
	}
}
