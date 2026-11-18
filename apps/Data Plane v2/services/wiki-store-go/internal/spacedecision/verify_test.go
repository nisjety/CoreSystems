package spacedecision

import (
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"time"
)

// testSigner stands in for Control's signing key. Its keys come from a fixed seed, so no key material is written into the
// source.
type testSigner struct {
	id   string
	priv ed25519.PrivateKey
}

func newTestSigner(label string) testSigner {
	seed := sha256.Sum256([]byte("wiki-store spacedecision test key " + label))
	return testSigner{id: "control-space-decision-" + label, priv: ed25519.NewKeyFromSeed(seed[:])}
}

func (s testSigner) public() ed25519.PublicKey {
	return s.priv.Public().(ed25519.PublicKey)
}

// signEnvelope builds version.key-id.payload.signature as Control's SignDecision does: base64url without padding for the key
// id, the payload and the signature, and Ed25519 over the first three parts.
func signEnvelope(t *testing.T, version string, signer testSigner, claims any) string {
	t.Helper()
	payload, err := json.Marshal(claims)
	if err != nil {
		t.Fatalf("marshal claims: %v", err)
	}
	return signRaw(version, signer, payload)
}

func signRaw(version string, signer testSigner, payload []byte) string {
	signed := version + "." +
		base64.RawURLEncoding.EncodeToString([]byte(signer.id)) + "." +
		base64.RawURLEncoding.EncodeToString(payload)
	return signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(signer.priv, []byte(signed)))
}

// requireRefusal fails unless err is a typed refusal carrying the wire code.
func requireRefusal(t *testing.T, err error, code string) {
	t.Helper()
	var refusal *Refusal
	if !errors.As(err, &refusal) {
		t.Fatalf("want a %s refusal, got %v", code, err)
	}
	if refusal.Code != code {
		t.Fatalf("refusal code = %s, want %s (%s)", refusal.Code, code, refusal.Message)
	}
}

// wellFormedClaims is a complete read decision for a room, valid at now.
func wellFormedClaims(now time.Time) Claims {
	return Claims{
		DecisionRef:               "decision-verify-1",
		OrgID:                     "org-1",
		SpaceRef:                  "space-room-1",
		SubjectID:                 "user-2",
		ServiceAudience:           AudienceCanvas,
		ActionID:                  ActionCanvasRead,
		ActionSchemaHash:          SchemaCanvasRead,
		PayloadDigest:             "sha256:" + strings.Repeat("a", 64),
		IdempotencyKey:            "canvas-read-verify-1",
		RecipientAudienceRef:      "audience:space-room-1:4",
		RecipientAudienceHash:     "sha256:audience-hash",
		PrivacyPolicyRef:          "privacy:org-1:5",
		ResourceAuthorizationRef:  "control:space-room-1:canvas:7",
		AuthorityRevision:         7,
		MembershipRevision:        3,
		PrivacyRevision:           5,
		RecipientAudienceRevision: 4,
		EntitlementRevision:       2,
		Permissions:               []string{PermissionCanvasRead},
		Purpose:                   "room canvas read",
		LawfulBasis:               "contract",
		PrivacyClass:              "room",
		RetentionClass:            "space",
		Residency:                 "eu",
		DeletionScope:             "space",
		IssuedAt:                  now.Add(-time.Minute),
		ExpiresAt:                 now.Add(time.Minute),
		Nonce:                     "nonce-verify-1",
		SpaceKind:                 "room",
	}
}

// wellFormedWriteClaims is wellFormedClaims for a canvas write.
func wellFormedWriteClaims(now time.Time) Claims {
	c := wellFormedClaims(now)
	c.ActionID = ActionCanvasWrite
	c.ActionSchemaHash = SchemaCanvasWrite
	c.Permissions = []string{PermissionCanvasWrite}
	return c
}

var verifyNow = time.Date(2026, time.November, 18, 12, 0, 0, 0, time.UTC)

func TestVerifyAcceptsAWellFormedSignedDecision(t *testing.T) {
	signer := newTestSigner("accept")
	keys := KeySet{signer.id: signer.public()}
	token := signEnvelope(t, EnvelopeVersion, signer, wellFormedClaims(verifyNow))

	claims, err := Verify(token, keys, verifyNow)
	if err != nil {
		t.Fatalf("a well-formed decision was refused: %v", err)
	}
	if claims.DecisionRef != "decision-verify-1" || claims.ActionID != ActionCanvasRead {
		t.Fatalf("verified claims lost their fields: %+v", claims)
	}
}

func TestVerifyRefusesADecisionOutsideItsLife(t *testing.T) {
	signer := newTestSigner("life")
	keys := KeySet{signer.id: signer.public()}

	expired := wellFormedClaims(verifyNow)
	expired.IssuedAt = verifyNow.Add(-2 * time.Minute)
	expired.ExpiresAt = verifyNow // expired at the instant it stops being valid
	_, err := Verify(signEnvelope(t, EnvelopeVersion, signer, expired), keys, verifyNow)
	requireRefusal(t, err, CodeDecisionInvalid)

	future := wellFormedClaims(verifyNow)
	future.IssuedAt = verifyNow.Add(5 * time.Minute)
	future.ExpiresAt = verifyNow.Add(7 * time.Minute)
	_, err = Verify(signEnvelope(t, EnvelopeVersion, signer, future), keys, verifyNow)
	requireRefusal(t, err, CodeDecisionInvalid)
}

func TestVerifyRefusesAnUntrustedKeyAndABadSignature(t *testing.T) {
	signer := newTestSigner("trusted")
	keys := KeySet{signer.id: signer.public()}
	claims := wellFormedClaims(verifyNow)

	stranger := newTestSigner("stranger")
	_, err := Verify(signEnvelope(t, EnvelopeVersion, stranger, claims), keys, verifyNow)
	requireRefusal(t, err, CodeDecisionInvalid)

	// The trusted key id, but a signature made with a different private key.
	forger := testSigner{id: signer.id, priv: stranger.priv}
	_, err = Verify(signEnvelope(t, EnvelopeVersion, forger, claims), keys, verifyNow)
	requireRefusal(t, err, CodeDecisionInvalid)

	// A valid envelope whose payload was changed after signing keeps the old signature and must fail.
	valid := strings.Split(signEnvelope(t, EnvelopeVersion, signer, claims), ".")
	tampered := claims
	tampered.OrgID = "org-other"
	tamperedPayload, err := json.Marshal(tampered)
	if err != nil {
		t.Fatalf("marshal tampered claims: %v", err)
	}
	valid[2] = base64.RawURLEncoding.EncodeToString(tamperedPayload)
	_, err = Verify(strings.Join(valid, "."), keys, verifyNow)
	requireRefusal(t, err, CodeDecisionInvalid)
}

func TestVerifyRefusesAMalformedEnvelope(t *testing.T) {
	signer := newTestSigner("malformed")
	keys := KeySet{signer.id: signer.public()}
	claims := wellFormedClaims(verifyNow)
	valid := signEnvelope(t, EnvelopeVersion, signer, claims)
	parts := strings.Split(valid, ".")

	cases := map[string]string{
		"empty":              "",
		"two parts":          "v2.abc",
		"three parts":        "v2." + parts[1] + "." + parts[2],
		"five parts":         valid + ".extra",
		"unknown version":    "v9." + parts[1] + "." + parts[2] + "." + parts[3],
		"undecodable key id": "v2.!!!." + parts[2] + "." + parts[3],
		"short signature":    strings.Join(parts[:3], ".") + "." + parts[3][:10],
		"payload not json":   signRaw(EnvelopeVersion, signer, []byte("not json")),
		"empty payload":      signRaw(EnvelopeVersion, signer, []byte{}),
	}
	for name, token := range cases {
		t.Run(name, func(t *testing.T) {
			_, err := Verify(token, keys, verifyNow)
			requireRefusal(t, err, CodeDecisionInvalid)
		})
	}
}

func TestVerifyRefusesARunActionEnvelope(t *testing.T) {
	signer := newTestSigner("run-version")
	keys := KeySet{signer.id: signer.public()}
	claims := wellFormedClaims(verifyNow)

	// The same signed claims verify under v2 and are refused under the run-action version, so the version alone decides.
	if _, err := Verify(signEnvelope(t, EnvelopeVersion, signer, claims), keys, verifyNow); err != nil {
		t.Fatalf("control: the v2 envelope with these claims was refused: %v", err)
	}
	runToken := signEnvelope(t, RunEnvelopeVersion, signer, claims)
	_, err := Verify(runToken, keys, verifyNow)
	requireRefusal(t, err, CodeDecisionInvalid)
}

func TestVerifyRefusesIncompleteClaims(t *testing.T) {
	signer := newTestSigner("incomplete")
	keys := KeySet{signer.id: signer.public()}
	cases := map[string]func(*Claims){
		"decision ref":   func(c *Claims) { c.DecisionRef = "" },
		"org":            func(c *Claims) { c.OrgID = "  " },
		"nonce":          func(c *Claims) { c.Nonce = "" },
		"authorization":  func(c *Claims) { c.ResourceAuthorizationRef = "" },
		"no permissions": func(c *Claims) { c.Permissions = nil },
	}
	for name, mutate := range cases {
		t.Run(name, func(t *testing.T) {
			claims := wellFormedClaims(verifyNow)
			mutate(&claims)
			_, err := Verify(signEnvelope(t, EnvelopeVersion, signer, claims), keys, verifyNow)
			requireRefusal(t, err, CodeDecisionInvalid)
		})
	}
}

func TestVerifyRefusesNonPositiveRevisions(t *testing.T) {
	signer := newTestSigner("revisions")
	keys := KeySet{signer.id: signer.public()}
	cases := map[string]func(*Claims){
		"authority zero":      func(c *Claims) { c.AuthorityRevision = 0 },
		"membership negative": func(c *Claims) { c.MembershipRevision = -1 },
		"privacy zero":        func(c *Claims) { c.PrivacyRevision = 0 },
		"audience zero":       func(c *Claims) { c.RecipientAudienceRevision = 0 },
		"entitlement zero":    func(c *Claims) { c.EntitlementRevision = 0 },
	}
	for name, mutate := range cases {
		t.Run(name, func(t *testing.T) {
			claims := wellFormedClaims(verifyNow)
			mutate(&claims)
			_, err := Verify(signEnvelope(t, EnvelopeVersion, signer, claims), keys, verifyNow)
			requireRefusal(t, err, CodeDecisionInvalid)
		})
	}
}

func TestVerifyRefusesAnOversizeEnvelope(t *testing.T) {
	signer := newTestSigner("oversize")
	keys := KeySet{signer.id: signer.public()}

	// A validly signed decision whose encoded form exceeds the envelope cap is refused on size, before any parsing.
	padded := wellFormedClaims(verifyNow)
	padded.Purpose = strings.Repeat("p", MaxEnvelopeBytes)
	token := signEnvelope(t, EnvelopeVersion, signer, padded)
	if len(token) <= MaxEnvelopeBytes {
		t.Fatalf("fixture is %d bytes, want more than %d", len(token), MaxEnvelopeBytes)
	}
	_, err := Verify(token, keys, verifyNow)
	requireRefusal(t, err, CodeDecisionInvalid)

	_, err = Verify(strings.Repeat("a", MaxEnvelopeBytes+1), keys, verifyNow)
	requireRefusal(t, err, CodeDecisionInvalid)
}

func TestParseKeySetAcceptsTheJSONMapAndTheSinglePair(t *testing.T) {
	first := newTestSigner("set-one")
	second := newTestSigner("set-two")
	encodedFirst := base64.RawURLEncoding.EncodeToString(first.public())
	encodedSecond := base64.RawURLEncoding.EncodeToString(second.public())

	rawJSON, err := json.Marshal(map[string]string{first.id: encodedFirst, second.id: encodedSecond})
	if err != nil {
		t.Fatalf("marshal key map: %v", err)
	}
	set, err := ParseKeySet(string(rawJSON), "", "")
	if err != nil {
		t.Fatalf("the JSON key map was refused: %v", err)
	}
	if len(set) != 2 || !set[first.id].Equal(first.public()) || !set[second.id].Equal(second.public()) {
		t.Fatalf("JSON key map parsed to %d keys, want both Control keys", len(set))
	}

	single, err := ParseKeySet("", first.id, encodedFirst)
	if err != nil {
		t.Fatalf("the single key pair was refused: %v", err)
	}
	if len(single) != 1 || !single[first.id].Equal(first.public()) {
		t.Fatalf("single key pair parsed to %d keys, want the one Control key", len(single))
	}
}

func TestParseKeySetRefusesAnEmptySet(t *testing.T) {
	signer := newTestSigner("empty")
	encoded := base64.RawURLEncoding.EncodeToString(signer.public())
	cases := map[string][3]string{
		"nothing configured":   {"", "", ""},
		"blank JSON":           {"   ", "", ""},
		"empty JSON map":       {"{}", "", ""},
		"key id without a key": {"", signer.id, ""},
		"key without a key id": {"", "", encoded},
	}
	for name, input := range cases {
		t.Run(name, func(t *testing.T) {
			if _, err := ParseKeySet(input[0], input[1], input[2]); err == nil {
				t.Fatalf("ParseKeySet(%q, %q, %q) accepted an empty or incomplete key set", input[0], input[1], input[2])
			}
		})
	}
}

func TestParseKeySetRefusesBadBase64AndAWrongKeyLength(t *testing.T) {
	signer := newTestSigner("encoding")
	cases := map[string][3]string{
		"not base64 (single)": {"", signer.id, "not*base64"},
		"31-byte key":         {"", signer.id, base64.RawURLEncoding.EncodeToString(make([]byte, 31))},
		"33-byte key":         {"", signer.id, base64.RawURLEncoding.EncodeToString(make([]byte, 33))},
		"bad base64 in map":   {`{"` + signer.id + `":"not*base64"}`, "", ""},
		"short key in map":    {`{"` + signer.id + `":"` + base64.RawURLEncoding.EncodeToString(make([]byte, 31)) + `"}`, "", ""},
	}
	for name, input := range cases {
		t.Run(name, func(t *testing.T) {
			if _, err := ParseKeySet(input[0], input[1], input[2]); err == nil {
				t.Fatalf("ParseKeySet accepted %s", name)
			}
		})
	}
}

func TestCheckCanvasAcceptsTheRoutesOwnDecision(t *testing.T) {
	now := verifyNow
	if err := wellFormedClaims(now).CheckCanvas("org-1", "user-2", "space-room-1", false); err != nil {
		t.Fatalf("a read decision for its own route was refused: %v", err)
	}
	if err := wellFormedWriteClaims(now).CheckCanvas("org-1", "user-2", "space-room-1", true); err != nil {
		t.Fatalf("a write decision for its own route was refused: %v", err)
	}
}

func TestCheckCanvasRefusesADecisionForAnotherCaller(t *testing.T) {
	now := verifyNow
	cases := map[string]struct {
		claims func() Claims
		check  func(Claims) error
		code   string
	}{
		"another org": {
			claims: func() Claims { return wellFormedClaims(now) },
			check: func(c Claims) error {
				return c.CheckCanvas("org-other", "user-2", "space-room-1", false)
			},
			code: CodeDecisionMismatch,
		},
		"another Space": {
			claims: func() Claims { return wellFormedClaims(now) },
			check: func(c Claims) error {
				return c.CheckCanvas("org-1", "user-2", "space-other", false)
			},
			code: CodeDecisionMismatch,
		},
		"another subject": {
			claims: func() Claims { return wellFormedClaims(now) },
			check: func(c Claims) error {
				return c.CheckCanvas("org-1", "user-other", "space-room-1", false)
			},
			code: CodeDecisionMismatch,
		},
		"another audience": {
			claims: func() Claims {
				c := wellFormedClaims(now)
				c.ServiceAudience = "documents-api"
				return c
			},
			check: func(c Claims) error {
				return c.CheckCanvas("org-1", "user-2", "space-room-1", false)
			},
			code: CodeDecisionMismatch,
		},
		"another action": {
			claims: func() Claims {
				c := wellFormedClaims(now)
				c.ActionID = "data.space.import"
				return c
			},
			check: func(c Claims) error {
				return c.CheckCanvas("org-1", "user-2", "space-room-1", false)
			},
			code: CodeDecisionMismatch,
		},
		"another schema": {
			claims: func() Claims {
				c := wellFormedClaims(now)
				c.ActionSchemaHash = SchemaCanvasWrite
				return c
			},
			check: func(c Claims) error {
				return c.CheckCanvas("org-1", "user-2", "space-room-1", false)
			},
			code: CodeDecisionMismatch,
		},
		"read token on a write route": {
			claims: func() Claims { return wellFormedClaims(now) },
			check: func(c Claims) error {
				return c.CheckCanvas("org-1", "user-2", "space-room-1", true)
			},
			code: CodeDecisionMismatch,
		},
		"missing canvas permission": {
			claims: func() Claims {
				c := wellFormedClaims(now)
				c.Permissions = []string{"space:read"}
				return c
			},
			check: func(c Claims) error {
				return c.CheckCanvas("org-1", "user-2", "space-room-1", false)
			},
			code: CodeDecisionMismatch,
		},
		"zero data retention": {
			claims: func() Claims {
				c := wellFormedClaims(now)
				c.ZeroDataRetention = true
				return c
			},
			check: func(c Claims) error {
				return c.CheckCanvas("org-1", "user-2", "space-room-1", false)
			},
			code: CodeDecisionMismatch,
		},
		"history floor": {
			claims: func() Claims {
				c := wellFormedClaims(now)
				c.HistoryFloorRevision = 3
				return c
			},
			check: func(c Claims) error {
				return c.CheckCanvas("org-1", "user-2", "space-room-1", false)
			},
			code: CodeHistoryFloor,
		},
		"dm kind": {
			claims: func() Claims {
				c := wellFormedClaims(now)
				c.SpaceKind = "dm"
				return c
			},
			check: func(c Claims) error {
				return c.CheckCanvas("org-1", "user-2", "space-room-1", false)
			},
			code: CodeDecisionMismatch,
		},
	}
	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			err := tc.check(tc.claims())
			requireRefusal(t, err, tc.code)
		})
	}
}
