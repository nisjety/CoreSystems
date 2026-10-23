//go:build e2e

package idp_test

import (
	"net/http/httptest"
	"testing"
	"time"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/e2e/idp"
	"github.com/triodelab/model-plane/services/browser-broker/internal/authz"
)

const issuer = "http://e2e-idp.invalid"

// The stub has to satisfy the broker's real verifier before any container is
// involved: this runs the same authctx.NewVerifier the broker calls at boot,
// against a JWKS served over HTTP, and the broker's own authz.Authorize.
func TestStubTokensAreAcceptedByTheBrokerVerifier(t *testing.T) {
	key, err := idp.Generate(idp.DefaultKID)
	if err != nil {
		t.Fatal(err)
	}
	srv := httptest.NewServer(idp.Handler(key.JWKS()))
	defer srv.Close()
	verifier, err := authctx.NewVerifier(authctx.Config{
		Audiences: []string{authz.Audience}, Issuer: issuer, JWKSURL: srv.URL + "/jwks.json",
	})
	if err != nil {
		t.Fatalf("verifier over the stub JWKS: %v", err)
	}

	person, _ := key.Sign(idp.Person(issuer, "org-1", "user-1"))
	p, err := verifier.Verify(person)
	if err != nil || p.PrincipalType != "user" || p.ActorID != "user-1" || p.OrganizationID != "org-1" {
		t.Fatalf("person token: %+v %v", p, err)
	}

	service, _ := key.Sign(idp.Service(issuer, "org-1", "model-gateway", authz.ScopeRead, authz.ScopeWrite))
	s, err := verifier.Verify(service)
	if err != nil || s.PrincipalType != "service" || !s.HasScope(authz.ScopeWrite) || !s.HasScope(authz.ScopeRead) {
		t.Fatalf("service token: %+v %v", s, err)
	}

	zdr := true
	spec := idp.Person(issuer, "org-1", "user-1")
	spec.ZDR = &zdr
	zdrToken, _ := key.Sign(spec)
	if z, err := verifier.Verify(zdrToken); err != nil || !z.ZeroDataRetention {
		t.Fatalf("zdr token: %+v %v", z, err)
	}
}

func TestStubBadTokensAreRefusedByTheBrokerVerifier(t *testing.T) {
	key, _ := idp.Generate(idp.DefaultKID)
	other, _ := idp.Generate(idp.DefaultKID) // same kid, different key
	srv := httptest.NewServer(idp.Handler(key.JWKS()))
	defer srv.Close()
	verifier, err := authctx.NewVerifier(authctx.Config{
		Audiences: []string{authz.Audience}, Issuer: issuer, JWKSURL: srv.URL + "/jwks.json",
	})
	if err != nil {
		t.Fatal(err)
	}
	wrongAudience := idp.Person(issuer, "org-1", "user-1")
	wrongAudience.Audience = "model-gateway"
	expired := idp.Person(issuer, "org-1", "user-1")
	expired.IssuedAt, expired.NotBefore, expired.Expires = time.Now().Add(-20*time.Minute), time.Now().Add(-20*time.Minute), time.Now().Add(-10*time.Minute)
	wrongIssuer := idp.Person("http://elsewhere.invalid", "org-1", "user-1")
	for name, c := range map[string]struct {
		signer *idp.Key
		spec   idp.Spec
	}{
		"wrong audience": {key, wrongAudience},
		"expired":        {key, expired},
		"wrong issuer":   {key, wrongIssuer},
		"foreign key":    {other, idp.Person(issuer, "org-1", "user-1")},
	} {
		raw, err := c.signer.Sign(c.spec)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := verifier.Verify(raw); err == nil {
			t.Fatalf("%s token was accepted", name)
		}
	}
}
