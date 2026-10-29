package eventauth

import (
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
)

func testKey(t *testing.T) (*rsa.PrivateKey, []byte) {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	return key, pem.EncodeToMemory(&pem.Block{Type: "RSA PRIVATE KEY", Bytes: x509.MarshalPKCS1PrivateKey(key)})
}

func TestSignerBindsProducerTenantUserZDRSubjectAndPayload(t *testing.T) {
	key, keyPEM := testKey(t)
	signer, err := NewSigner(keyPEM, "service:documents-api-go", "documents-events-v1", "dataplane-events", "events:documents:publish")
	if err != nil {
		t.Fatal(err)
	}
	payload := []byte(`{"document_id":"doc-test","org_id":"org-test","user_id":"user-test","zdr":true}`)
	envelopeBytes, err := signer.Sign("dataplane.documents.created", payload)
	if err != nil {
		t.Fatal(err)
	}
	var envelope struct {
		Authorization string `json:"authorization"`
		Data          string `json:"data"`
	}
	if err := json.Unmarshal(envelopeBytes, &envelope); err != nil {
		t.Fatal(err)
	}
	if envelope.Authorization[:7] != "Bearer " {
		t.Fatal("missing Bearer scheme")
	}
	parsed, err := jwt.Parse(envelope.Authorization[7:], func(token *jwt.Token) (any, error) {
		return &key.PublicKey, nil
	}, jwt.WithValidMethods([]string{"RS256"}), jwt.WithAudience("dataplane-events"), jwt.WithIssuer("service:documents-api-go"))
	if err != nil || !parsed.Valid {
		t.Fatalf("verify token: %v", err)
	}
	claims := parsed.Claims.(jwt.MapClaims)
	for key, want := range map[string]any{
		"sub": "service:documents-api-go", "principal_type": "service",
		"org_id": "org-test", "user_id": "user-test", "zdr": true,
		"event_type": "dataplane.documents.created",
	} {
		if got := claims[key]; got != want {
			t.Fatalf("%s=%v, want %v", key, got, want)
		}
	}
	if claims["jti"] == "" || claims["payload_sha256"] == "" {
		t.Fatal("missing replay or payload binding")
	}
	if exp := int64(claims["exp"].(float64)); exp <= time.Now().Unix() || exp > time.Now().Add(5*time.Minute).Unix() {
		t.Fatalf("invalid bounded expiry: %d", exp)
	}
	decoded, err := base64.RawURLEncoding.DecodeString(envelope.Data)
	if err != nil || string(decoded) != string(payload) {
		t.Fatalf("payload mismatch: %v", err)
	}
}

func TestSignerRejectsAmbiguousOrConflictingBoundaryFields(t *testing.T) {
	_, keyPEM := testKey(t)
	invalidConfigs := [][4]string{
		{"documents-api-go", "kid", "aud", "scope"},
		{"service:documents-api-go", "", "aud", "scope"},
		{"service:documents-api-go", "kid", "", "scope"},
		{"service:documents-api-go", "kid", "aud", ""},
	}
	for _, cfg := range invalidConfigs {
		if _, err := NewSigner(keyPEM, cfg[0], cfg[1], cfg[2], cfg[3]); err == nil {
			t.Fatalf("accepted invalid config: %#v", cfg)
		}
	}
	signer, err := NewSigner(keyPEM, "service:documents-api-go", "kid", "aud", "scope")
	if err != nil {
		t.Fatal(err)
	}
	for _, payload := range [][]byte{
		{}, []byte(`[]`), []byte(`{"org_id":""}`),
		[]byte(`{"org_id":"org-test","user_id":""}`),
		[]byte(`{"org_id":"org-test","zdr":"false"}`),
	} {
		if _, err := signer.Sign("dataplane.documents.created", payload); err == nil {
			t.Fatalf("accepted invalid payload: %s", payload)
		}
	}
	if _, err := signer.Sign("", []byte(`{"org_id":"org-test","zdr":false}`)); err == nil {
		t.Fatal("accepted empty event type")
	}
	if _, err := signer.Sign("dataplane.search.rebuild.requested", []byte(`{"org_id":"org-test","zdr":false}`)); err == nil {
		t.Fatal("documents producer signed an unauthorized subject")
	}
}

func TestSignerAcceptsPKCS8AndRejectsMalformedWeakOrNonRSAKeys(t *testing.T) {
	key, _ := testKey(t)
	pkcs8, err := x509.MarshalPKCS8PrivateKey(key)
	if err != nil {
		t.Fatal(err)
	}
	pkcs8PEM := pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: pkcs8})
	if _, err := NewSigner(pkcs8PEM, "service:documents-api-go", "kid", "aud", "scope"); err != nil {
		t.Fatalf("valid PKCS8 RSA key rejected: %v", err)
	}

	weak, err := rsa.GenerateKey(rand.Reader, 1024)
	if err != nil {
		t.Fatal(err)
	}
	ecdsaKey, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	ecdsaPKCS8, err := x509.MarshalPKCS8PrivateKey(ecdsaKey)
	if err != nil {
		t.Fatal(err)
	}
	invalidKeys := [][]byte{
		[]byte("not pem"),
		pem.EncodeToMemory(&pem.Block{Type: "RSA PRIVATE KEY", Bytes: []byte("not der")}),
		pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: ecdsaPKCS8}),
		pem.EncodeToMemory(&pem.Block{Type: "RSA PRIVATE KEY", Bytes: x509.MarshalPKCS1PrivateKey(weak)}),
	}
	for _, invalid := range invalidKeys {
		if _, err := NewSigner(invalid, "service:documents-api-go", "kid", "aud", "scope"); err == nil {
			t.Fatal("invalid event signing key accepted")
		}
	}
}
