package eventauth

import (
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/json"
	"encoding/pem"
	"testing"

	"github.com/golang-jwt/jwt/v5"
)

func testPrivateKeyPEM(t *testing.T) (*rsa.PrivateKey, []byte) {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	return key, pem.EncodeToMemory(&pem.Block{
		Type:  "RSA PRIVATE KEY",
		Bytes: x509.MarshalPKCS1PrivateKey(key),
	})
}

func TestSignerBindsWikiProducerTenantUserZDRSubjectAndPayload(t *testing.T) {
	key, keyPEM := testPrivateKeyPEM(t)
	signer, err := NewSigner(keyPEM)
	if err != nil {
		t.Fatal(err)
	}
	payload := []byte(`{"page_id":"page-test","version_id":"version-test","org_id":"org-test","user_id":"user-test","zdr":false}`)
	wire, err := signer.Sign(SubjectWikiPublished, payload)
	if err != nil {
		t.Fatal(err)
	}

	var envelope struct {
		Authorization string `json:"authorization"`
		Data          string `json:"data"`
	}
	if err := json.Unmarshal(wire, &envelope); err != nil {
		t.Fatal(err)
	}
	if len(envelope.Authorization) < 8 || envelope.Authorization[:7] != "Bearer " {
		t.Fatal("missing bearer envelope")
	}
	parsed, err := jwt.Parse(envelope.Authorization[7:], func(*jwt.Token) (any, error) {
		return &key.PublicKey, nil
	}, jwt.WithValidMethods([]string{"RS256"}), jwt.WithAudience(Audience), jwt.WithIssuer(Issuer))
	if err != nil || !parsed.Valid {
		t.Fatalf("verify token: %v", err)
	}
	claims := parsed.Claims.(jwt.MapClaims)
	for key, want := range map[string]any{
		"sub": Issuer, "principal_type": "service", "org_id": "org-test",
		"user_id": "user-test", "zdr": false, "event_type": SubjectWikiPublished,
	} {
		if got := claims[key]; got != want {
			t.Fatalf("%s=%v, want %v", key, got, want)
		}
	}
	if claims["payload_sha256"] == "" || claims["jti"] == "" {
		t.Fatal("missing payload or replay binding")
	}
}

func TestSignerRejectsMissingOrConflictingBoundaryFieldsAndWrongSubject(t *testing.T) {
	_, keyPEM := testPrivateKeyPEM(t)
	signer, err := NewSigner(keyPEM)
	if err != nil {
		t.Fatal(err)
	}
	for _, payload := range [][]byte{
		nil,
		[]byte(`[]`),
		[]byte(`{"org_id":"","zdr":false}`),
		[]byte(`{"org_id":"org-test"}`),
		[]byte(`{"org_id":"org-test","user_id":"","zdr":false}`),
		[]byte(`{"org_id":"org-test","zdr":true}`),
	} {
		if _, err := signer.Sign(SubjectWikiPublished, payload); err == nil {
			t.Fatalf("accepted invalid durable wiki event: %s", payload)
		}
	}
	if _, err := signer.Sign("dataplane.documents.created", []byte(`{"org_id":"org-test","zdr":false}`)); err == nil {
		t.Fatal("wiki signer accepted unauthorized subject")
	}
}

func TestSignerRejectsMissingMalformedAndWeakKeys(t *testing.T) {
	weak, err := rsa.GenerateKey(rand.Reader, 1024)
	if err != nil {
		t.Fatal(err)
	}
	for _, key := range [][]byte{
		nil,
		[]byte("not pem"),
		pem.EncodeToMemory(&pem.Block{Type: "RSA PRIVATE KEY", Bytes: x509.MarshalPKCS1PrivateKey(weak)}),
	} {
		if _, err := NewSigner(key); err == nil {
			t.Fatal("accepted invalid signing key")
		}
	}
}

func TestSignerAcceptsPKCS8RSAAndRejectsNonRSAKey(t *testing.T) {
	key, _ := testPrivateKeyPEM(t)
	pkcs8, err := x509.MarshalPKCS8PrivateKey(key)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := NewSigner(pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: pkcs8})); err != nil {
		t.Fatalf("valid PKCS8 RSA key rejected: %v", err)
	}

	ecdsaKey, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	ecdsaPKCS8, err := x509.MarshalPKCS8PrivateKey(ecdsaKey)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := NewSigner(pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: ecdsaPKCS8})); err == nil {
		t.Fatal("non-RSA event signing key accepted")
	}
}
