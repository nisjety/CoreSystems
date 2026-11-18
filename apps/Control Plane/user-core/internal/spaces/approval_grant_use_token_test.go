package spaces

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// WS10-S2: the approval-grant-use token. It is its own envelope version, so no
// recipient can take it for a run-action-v1 token or a Space thread decision.

func validApprovalRule() ApprovalRule {
	return ApprovalRule{
		RuleID: "rule-1", OrgID: "org-1", SpaceRef: "space-room", Scope: "always",
		AgentSubjectID: "agent-1", ToolKey: "mcp__github__list_issues", ActionKey: "mcp__github__list_issues",
		ActionSchemaHash:   "sha256:" + strings.Repeat("a", 64),
		GrantedBySubjectID: "user-manager", GrantedByRole: "manager", AuthorityRevision: 7,
	}
}

func validApprovalUse(t *testing.T, now time.Time) ApprovalGrantUse {
	t.Helper()
	evidence := validPersonalThreadEvidence()
	evidence.Membership.Kind = KindRoom
	evidence.Membership.SpaceRef = "space-room"
	evidence.Membership.Role = "editor"
	use, err := NewApprovalGrantUse(evidence, validApprovalRule(), "run-1", "token-ref-1", "nonce-1", now)
	if err != nil {
		t.Fatalf("NewApprovalGrantUse: %v", err)
	}
	return use
}

func resignApprovalUse(t *testing.T, key SigningKey, use ApprovalGrantUse) string {
	t.Helper()
	payload, err := json.Marshal(use)
	if err != nil {
		t.Fatal(err)
	}
	signed := approvalGrantUseVersion + "." + base64.RawURLEncoding.EncodeToString([]byte(key.ID)) + "." + base64.RawURLEncoding.EncodeToString(payload)
	return signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(key.PrivateKey, []byte(signed)))
}

func TestApprovalGrantUseSignVerifyRoundTrip(t *testing.T) {
	key, _ := testDecisionKeys(t)
	now := time.Date(2026, 11, 2, 9, 0, 0, 0, time.UTC)
	use := validApprovalUse(t, now)
	if use.SubjectID != "user-1" || use.SpaceRef != "space-room" || use.AgentSubjectID != "agent-1" ||
		use.Audience != "model-plane" || use.Scope != "always" || use.AuthorityRevision != 7 ||
		use.ExpiresAt.Sub(use.IssuedAt) != ApprovalGrantUseLifetime {
		t.Fatalf("claims are not built from the rule and evidence: %#v", use)
	}
	token, err := SignApprovalGrantUse(key, use)
	if err != nil {
		t.Fatalf("SignApprovalGrantUse: %v", err)
	}
	if !strings.HasPrefix(token, approvalGrantUseVersion+".") || strings.Count(token, ".") != 3 {
		t.Fatalf("unexpected envelope: %q", token)
	}
	got, err := VerifyApprovalGrantUse(key, token, now.Add(30*time.Second))
	if err != nil {
		t.Fatalf("VerifyApprovalGrantUse: %v", err)
	}
	if got != use {
		t.Fatalf("round trip changed the claims:\n got %#v\nwant %#v", got, use)
	}
}

func TestApprovalGrantUseVerifyRefusesWrongKeyTamperingAndExpiry(t *testing.T) {
	key, _ := testDecisionKeys(t)
	otherKey, _ := testDecisionKeys(t)
	now := time.Date(2026, 11, 2, 9, 0, 0, 0, time.UTC)
	use := validApprovalUse(t, now)
	token, err := SignApprovalGrantUse(key, use)
	if err != nil {
		t.Fatal(err)
	}

	t.Run("a different key with the same id", func(t *testing.T) {
		if _, err := VerifyApprovalGrantUse(otherKey, token, now); err == nil {
			t.Fatal("token verified under a different key")
		}
	})
	t.Run("a different key id", func(t *testing.T) {
		renamed := key
		renamed.ID = "control-other"
		if _, err := VerifyApprovalGrantUse(renamed, token, now); err == nil {
			t.Fatal("token verified under a different key id")
		}
	})
	t.Run("tampered payload", func(t *testing.T) {
		parts := strings.Split(token, ".")
		forged := use
		forged.ToolKey, forged.ActionKey = "mcp__other__tool", "mcp__other__tool"
		raw, _ := json.Marshal(forged)
		parts[2] = base64.RawURLEncoding.EncodeToString(raw)
		if _, err := VerifyApprovalGrantUse(key, strings.Join(parts, "."), now); err == nil {
			t.Fatal("a token with an altered payload verified")
		}
	})
	t.Run("tampered signature", func(t *testing.T) {
		parts := strings.Split(token, ".")
		sig, _ := base64.RawURLEncoding.DecodeString(parts[3])
		sig[0] ^= 0xff
		parts[3] = base64.RawURLEncoding.EncodeToString(sig)
		if _, err := VerifyApprovalGrantUse(key, strings.Join(parts, "."), now); err == nil {
			t.Fatal("a token with an altered signature verified")
		}
	})
	t.Run("expired", func(t *testing.T) {
		if _, err := VerifyApprovalGrantUse(key, token, now.Add(ApprovalGrantUseLifetime+time.Second)); err == nil {
			t.Fatal("an expired token verified")
		}
		if _, err := VerifyApprovalGrantUse(key, token, now.Add(ApprovalGrantUseLifetime-time.Second)); err != nil {
			t.Fatalf("a token one second before expiry was refused: %v", err)
		}
	})
	t.Run("issued in the future", func(t *testing.T) {
		if _, err := VerifyApprovalGrantUse(key, token, now.Add(-2*time.Minute)); err == nil {
			t.Fatal("a token issued well after the verification time verified")
		}
	})
	t.Run("lifetime over two minutes", func(t *testing.T) {
		long := use
		long.ExpiresAt = use.IssuedAt.Add(ApprovalGrantUseLifetime + time.Second)
		if _, err := SignApprovalGrantUse(key, long); err == nil {
			t.Fatal("signing accepted a lifetime over two minutes")
		}
		// A correctly signed token with an overlong life is still refused on verify.
		if _, err := VerifyApprovalGrantUse(key, resignApprovalUse(t, key, long), now); err == nil {
			t.Fatal("verifying accepted a signed token with a lifetime over two minutes")
		}
	})
	t.Run("scope other than always", func(t *testing.T) {
		for _, scope := range []string{"once", "thread", ""} {
			bad := use
			bad.Scope = scope
			if _, err := SignApprovalGrantUse(key, bad); err == nil {
				t.Errorf("signing accepted scope %q", scope)
			}
			if _, err := VerifyApprovalGrantUse(key, resignApprovalUse(t, key, bad), now); err == nil {
				t.Errorf("verifying accepted a signed token with scope %q", scope)
			}
		}
	})
	t.Run("unknown claim", func(t *testing.T) {
		parts := strings.Split(token, ".")
		raw, _ := json.Marshal(use)
		var m map[string]any
		_ = json.Unmarshal(raw, &m)
		m["extra"] = "x"
		raw, _ = json.Marshal(m)
		parts[2] = base64.RawURLEncoding.EncodeToString(raw)
		signed := strings.Join(parts[:3], ".")
		parts[3] = base64.RawURLEncoding.EncodeToString(ed25519.Sign(key.PrivateKey, []byte(signed)))
		if _, err := VerifyApprovalGrantUse(key, strings.Join(parts, "."), now); err == nil {
			t.Fatal("a signed token with an unknown claim verified")
		}
	})
}

func TestApprovalGrantUseValidateRequiresEveryClaim(t *testing.T) {
	now := time.Date(2026, 11, 2, 9, 0, 0, 0, time.UTC)
	for name, mutate := range map[string]func(*ApprovalGrantUse){
		"token_ref":           func(u *ApprovalGrantUse) { u.TokenRef = " " },
		"rule_id":             func(u *ApprovalGrantUse) { u.RuleID = "" },
		"run_id":              func(u *ApprovalGrantUse) { u.RunID = "" },
		"org_id":              func(u *ApprovalGrantUse) { u.OrgID = "" },
		"space_ref":           func(u *ApprovalGrantUse) { u.SpaceRef = "" },
		"subject_id":          func(u *ApprovalGrantUse) { u.SubjectID = "" },
		"agent_subject_id":    func(u *ApprovalGrantUse) { u.AgentSubjectID = "" },
		"tool_key":            func(u *ApprovalGrantUse) { u.ToolKey = "" },
		"action_key":          func(u *ApprovalGrantUse) { u.ActionKey = "" },
		"nonce":               func(u *ApprovalGrantUse) { u.Nonce = "" },
		"audience":            func(u *ApprovalGrantUse) { u.Audience = "gateway" },
		"schema hash":         func(u *ApprovalGrantUse) { u.ActionSchemaHash = "sha256:short" },
		"authority revision":  func(u *ApprovalGrantUse) { u.AuthorityRevision = 0 },
		"expiry before issue": func(u *ApprovalGrantUse) { u.ExpiresAt = u.IssuedAt.Add(-time.Second) },
		"zero issue":          func(u *ApprovalGrantUse) { u.IssuedAt = time.Time{} },
	} {
		t.Run(name, func(t *testing.T) {
			use := validApprovalUse(t, now)
			mutate(&use)
			if err := use.Validate(); err == nil {
				t.Fatalf("%s mutation validated", name)
			}
		})
	}
	if _, err := NewApprovalGrantUse(validPersonalThreadEvidence(), validApprovalRule(), "run-1", "ref", "nonce", time.Time{}); err == nil {
		t.Fatal("a zero issuance time was accepted")
	}
	if _, err := SignApprovalGrantUse(SigningKey{}, validApprovalUse(t, now)); err == nil {
		t.Fatal("an empty signing key was accepted")
	}
}

// A token minted for one envelope must not be accepted as another, in either
// direction, even when it is correctly signed with the same Control key.
func TestApprovalGrantUseIsNotInterchangeableWithOtherEnvelopes(t *testing.T) {
	key, verification := testDecisionKeys(t)
	now := time.Date(2026, 11, 2, 9, 0, 0, 0, time.UTC)
	use := validApprovalUse(t, now)
	token, err := SignApprovalGrantUse(key, use)
	if err != nil {
		t.Fatal(err)
	}

	// A grant-use token is neither a Space decision nor a run-action decision.
	if _, err := VerifyDecision(verification, token, expectedDecision(now)); err == nil {
		t.Fatal("a Space decision verifier accepted an approval-grant-use token")
	}
	if _, err := VerifyRunActionDecision(key, token, now); err == nil {
		t.Fatal("the run-action verifier accepted an approval-grant-use token")
	}

	// A run-action decision is not a grant-use token.
	evidence := validPersonalThreadEvidence()
	evidence.AgentActionEntitled = true
	evidence.ResourceAuthorizationRef = "control:space-personal:thread-create:7"
	decision, err := IssueRunActionDecision(evidence, validRunActionAuthority(), validRunActionRequest(), now)
	if err != nil {
		t.Fatalf("IssueRunActionDecision: %v", err)
	}
	decisionToken, err := SignRunActionDecision(key, decision)
	if err != nil {
		t.Fatalf("SignRunActionDecision: %v", err)
	}
	if _, err := VerifyApprovalGrantUse(key, decisionToken, now); err == nil {
		t.Fatal("VerifyApprovalGrantUse accepted a signed run-action decision")
	}

	// A thread decision is not a grant-use token either.
	threadDecision, err := IssuePersonalThreadCreateDecision(validPersonalThreadEvidence(), validPersonalThreadRequest(), now)
	if err != nil {
		t.Fatalf("IssuePersonalThreadCreateDecision: %v", err)
	}
	threadToken, err := SignDecision(key, threadDecision)
	if err != nil {
		t.Fatalf("SignDecision: %v", err)
	}
	if _, err := VerifyApprovalGrantUse(key, threadToken, now); err == nil {
		t.Fatal("VerifyApprovalGrantUse accepted a signed Space thread decision")
	}

	// Swapping only the version label breaks the signature.
	relabelled := strings.Replace(token, approvalGrantUseVersion, "run-action-v1", 1)
	if _, err := VerifyApprovalGrantUse(key, relabelled, now); err == nil {
		t.Fatal("a relabelled token verified as a grant-use token")
	}
}

// approvalGrantUseGoldenVector is the shape of testdata/approval-grant-use-v1.golden.json.
type approvalGrantUseGoldenVector struct {
	SigningKey struct {
		ID           string `json:"id"`
		SeedHex      string `json:"seed_hex"`
		PublicKeyHex string `json:"public_key_hex"`
	} `json:"signing_key"`
	Input struct {
		TokenRef          string `json:"token_ref"`
		RuleID            string `json:"rule_id"`
		RunID             string `json:"run_id"`
		Nonce             string `json:"nonce"`
		OrgID             string `json:"org_id"`
		SpaceRef          string `json:"space_ref"`
		SubjectID         string `json:"subject_id"`
		AgentSubjectID    string `json:"agent_subject_id"`
		ToolKey           string `json:"tool_key"`
		ActionKey         string `json:"action_key"`
		ActionSchemaHash  string `json:"action_schema_hash"`
		AuthorityRevision int64  `json:"authority_revision"`
		IssuedAt          string `json:"issued_at"`
	} `json:"input"`
	Expected struct {
		Token string `json:"token"`
	} `json:"expected"`
}

// WS10-S3: a Control-signed approval-grant-use-v1 token must verify in Session
// Core. This is the Control side of that cross-vector: it pins the exact
// token bytes Session Core's own verifier must accept and the exact claims it
// must recover, under a fixed Ed25519 seed shared with the Rust fixture named
// in the golden file's comment. It does not and cannot run the Rust half from
// here - that assertion belongs to the session-core area.
func TestApprovalGrantUseGoldenVectorMatchesSessionCore(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join("testdata", "approval-grant-use-v1.golden.json"))
	if err != nil {
		t.Fatalf("read golden: %v", err)
	}
	var golden approvalGrantUseGoldenVector
	if err := json.Unmarshal(raw, &golden); err != nil {
		t.Fatalf("decode golden: %v", err)
	}

	seed, err := hex.DecodeString(golden.SigningKey.SeedHex)
	if err != nil || len(seed) != ed25519.SeedSize {
		t.Fatalf("golden seed_hex is invalid: %v", err)
	}
	key := SigningKey{ID: golden.SigningKey.ID, PrivateKey: ed25519.NewKeyFromSeed(seed)}
	if got := hex.EncodeToString(key.PrivateKey.Public().(ed25519.PublicKey)); got != golden.SigningKey.PublicKeyHex {
		t.Fatalf("public key = %s, golden %s", got, golden.SigningKey.PublicKeyHex)
	}

	issuedAt, err := time.Parse(time.RFC3339, golden.Input.IssuedAt)
	if err != nil {
		t.Fatalf("parse issued_at: %v", err)
	}
	in := golden.Input
	evidence := validPersonalThreadEvidence()
	evidence.Membership.Kind = KindRoom
	evidence.Membership.OrgID = in.OrgID
	evidence.Membership.SpaceRef = in.SpaceRef
	evidence.Membership.SubjectID = in.SubjectID
	evidence.Membership.Revisions.Authority = in.AuthorityRevision
	rule := ApprovalRule{
		RuleID: in.RuleID, OrgID: in.OrgID, SpaceRef: in.SpaceRef, Scope: string(ApprovalScopeAlways),
		AgentSubjectID: in.AgentSubjectID, ToolKey: in.ToolKey, ActionKey: in.ActionKey,
		ActionSchemaHash: in.ActionSchemaHash, AuthorityRevision: in.AuthorityRevision,
	}
	use, err := NewApprovalGrantUse(evidence, rule, in.RunID, in.TokenRef, in.Nonce, issuedAt)
	if err != nil {
		t.Fatalf("NewApprovalGrantUse: %v", err)
	}

	token, err := SignApprovalGrantUse(key, use)
	if err != nil {
		t.Fatalf("SignApprovalGrantUse: %v", err)
	}
	if token != golden.Expected.Token {
		t.Fatalf("token = %s, golden %s (change only on an intended wire change, on both sides)", token, golden.Expected.Token)
	}

	// Session Core's own verifier must recover exactly this claim set from
	// exactly this token; this proves the Go side of that promise.
	verified, err := VerifyApprovalGrantUse(key, golden.Expected.Token, issuedAt.Add(time.Second))
	if err != nil {
		t.Fatalf("VerifyApprovalGrantUse: %v", err)
	}
	if verified != use {
		t.Fatalf("verified claims = %+v, want %+v", verified, use)
	}
}
