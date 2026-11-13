package crypto

import (
	"encoding/base64"
	"errors"
	"strings"
	"testing"
	"time"
)

// Credential references (W4-CC-4): crf1.<base64url(nonce || AES-GCM ciphertext)> under a key derived
// from the vault key with the purpose label credential-ref-v1. The reference carries its scope and no
// secret; a holder cannot change the scope, and an authentic reference for another host or call says so.

var credRefVaultKey = []byte("12345678901234567890123456789012")

func newRefsForTest(t *testing.T, key []byte) *CredentialRefs {
	t.Helper()
	refs, err := NewCredentialRefs(key)
	if err != nil {
		t.Fatalf("NewCredentialRefs: %v", err)
	}
	return refs
}

func TestACredentialRefRoundTripsItsScopeAndHoldsItInCiphertextOnly(t *testing.T) {
	refs := newRefsForTest(t, credRefVaultKey)
	expires := time.Date(2026, 11, 12, 10, 0, 30, 900_000_000, time.UTC)
	ref, err := refs.Seal("cb_123", "org-1", "conn-1", "call-1", "api_key", "api.example.com", expires)
	if err != nil {
		t.Fatalf("Seal: %v", err)
	}
	if !strings.HasPrefix(ref, "crf1.") {
		t.Fatalf("ref %q, want the crf1. prefix", ref)
	}
	payload, err := base64.RawURLEncoding.DecodeString(strings.TrimPrefix(ref, "crf1."))
	if err != nil {
		t.Fatalf("the ref body is not unpadded base64url: %v", err)
	}
	for _, field := range []string{"cb_123", "org-1", "conn-1", "call-1", "api_key", "api.example.com"} {
		if strings.Contains(string(payload), field) || strings.Contains(ref, field) {
			t.Fatalf("the reference shows %q in clear", field)
		}
	}
	claims, err := refs.Open(ref, "api.example.com", "call-1")
	if err != nil {
		t.Fatalf("Open: %v", err)
	}
	want := CredentialRefClaims{
		BindingID: "cb_123", OrgID: "org-1", ConnectionID: "conn-1", CallID: "call-1", Slot: "api_key",
		Host: "api.example.com", ExpiresAt: time.Date(2026, 11, 12, 10, 0, 30, 0, time.UTC),
	}
	if claims != want {
		t.Fatalf("claims %+v, want %+v (expiry stored to the second, never later than asked)", claims, want)
	}

	again, err := refs.Seal("cb_123", "org-1", "conn-1", "call-1", "api_key", "api.example.com", expires)
	if err != nil || again == ref {
		t.Fatalf("two seals of one scope gave the same reference (%v): the nonce is not fresh", err)
	}
}

func TestAnAuthenticReferenceForAnotherHostOrCallIsRefusedWithItsClaims(t *testing.T) {
	refs := newRefsForTest(t, credRefVaultKey)
	ref, err := refs.Seal("cb_123", "org-1", "", "call-1", "api_key", "api.example.com", time.Now().Add(time.Minute))
	if err != nil {
		t.Fatal(err)
	}
	claims, err := refs.Open(ref, "evil.example.com", "call-1")
	if !errors.Is(err, ErrCredentialRefHostMismatch) || claims.BindingID != "cb_123" || claims.OrgID != "org-1" {
		t.Fatalf("another host: claims %+v, err %v; want the host mismatch with the claims (so the refusal can be audited)", claims, err)
	}
	claims, err = refs.Open(ref, "api.example.com", "call-2")
	if !errors.Is(err, ErrCredentialRefCallMismatch) || claims.BindingID != "cb_123" {
		t.Fatalf("another call: claims %+v, err %v; want the call mismatch with the claims", claims, err)
	}
	if claims, err := refs.Open(ref, "api.example.com", "call-1"); err != nil || claims.ConnectionID != "" {
		t.Fatalf("an empty connection id must round-trip as empty: %+v %v", claims, err)
	}
}

func TestAReferenceThatDoesNotAuthenticateNamesNothing(t *testing.T) {
	refs := newRefsForTest(t, credRefVaultKey)
	ref, err := refs.Seal("cb_123", "org-1", "conn-1", "call-1", "api_key", "api.example.com", time.Now().Add(time.Minute))
	if err != nil {
		t.Fatal(err)
	}
	payload, _ := base64.RawURLEncoding.DecodeString(strings.TrimPrefix(ref, "crf1."))
	flipped := append([]byte(nil), payload...)
	flipped[len(flipped)/2] ^= 0x01
	otherKey, err := newRefsForTest(t, []byte("abcdefghijklmnopqrstuvwxyz012345")).Seal("cb_123", "org-1", "conn-1", "call-1", "api_key", "api.example.com", time.Now().Add(time.Minute))
	if err != nil {
		t.Fatal(err)
	}
	vault, err := NewVault(credRefVaultKey)
	if err != nil {
		t.Fatal(err)
	}
	vaultCiphertext, err := vault.Encrypt("v1|cb_123|org-1|conn-1|call-1|api_key|api.example.com|9999999999", []byte("credential-ref-v1"))
	if err != nil {
		t.Fatal(err)
	}
	for name, bad := range map[string]string{
		"a flipped byte":              "crf1." + base64.RawURLEncoding.EncodeToString(flipped),
		"another key":                 otherKey,
		"another prefix":              "crf2." + strings.TrimPrefix(ref, "crf1."),
		"no prefix":                   strings.TrimPrefix(ref, "crf1."),
		"padded base64":               ref + "==",
		"too short":                   "crf1.AAAA",
		"empty":                       "",
		"oversized":                   "crf1." + strings.Repeat("A", 9000),
		"the raw vault key's sealing": "crf1." + base64.RawURLEncoding.EncodeToString(mustDecodeVault(t, vaultCiphertext)),
	} {
		claims, err := refs.Open(bad, "api.example.com", "call-1")
		if !errors.Is(err, ErrCredentialRefInvalid) || claims != (CredentialRefClaims{}) {
			t.Errorf("%s: Open gave claims %+v, err %v; want ErrCredentialRefInvalid and no claims", name, claims, err)
		}
	}
}

// mustDecodeVault returns the nonce||ciphertext of a vault ciphertext: the vault key itself, not the
// derived reference key, sealed it, so it must not open as a reference.
func mustDecodeVault(t *testing.T, ciphertext string) []byte {
	t.Helper()
	raw, err := base64.StdEncoding.DecodeString(strings.TrimPrefix(ciphertext, "v1:"))
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

func TestSealRefusesAScopeFieldThatCouldForgeAnother(t *testing.T) {
	refs := newRefsForTest(t, credRefVaultKey)
	expires := time.Now().Add(time.Minute)
	valid := []string{"cb_123", "org-1", "conn-1", "call-1", "api_key", "api.example.com"}
	for index := range valid {
		for _, bad := range []string{"a|b", "a b", "a\nb", strings.Repeat("x", 257), "\x7f"} {
			fields := append([]string(nil), valid...)
			fields[index] = bad
			if _, err := refs.Seal(fields[0], fields[1], fields[2], fields[3], fields[4], fields[5], expires); err == nil {
				t.Errorf("field %d = %q was sealed", index, bad)
			}
		}
		if index == 2 {
			continue // only the connection id may be empty
		}
		fields := append([]string(nil), valid...)
		fields[index] = ""
		if _, err := refs.Seal(fields[0], fields[1], fields[2], fields[3], fields[4], fields[5], expires); err == nil {
			t.Errorf("an empty field %d was sealed", index)
		}
	}
	if _, err := refs.Seal("cb_123", "org-1", "conn-1", "call-1", "api_key", "api.example.com", time.Time{}); err == nil {
		t.Error("a reference without an expiry was sealed")
	}
	if _, err := NewCredentialRefs(nil); err == nil {
		t.Error("references were built with no key material")
	}
}
