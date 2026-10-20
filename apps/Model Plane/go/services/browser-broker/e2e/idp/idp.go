//go:build e2e

// Package idp is the stub identity provider of the browser-broker end-to-end
// harness: an RSA key generated at run time, the JWKS document the broker
// fetches once at boot (authctx.NewVerifier), and an RS256 signer that issues
// the tokens the broker's verifier and authz rules accept.
//
// Nothing here is a credential of any real system. The key never exists before
// a run starts and is never committed; the private half stays in a temporary
// directory on the host, only the public JWKS goes into the stub's image.
package idp

import (
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"errors"
	"fmt"
	"math/big"
	"net/http"
	"os"
	"time"

	"github.com/golang-jwt/jwt/v5"
)

const (
	// Audience is the only audience the broker accepts (authz.Audience).
	Audience = "browser-broker"
	// DefaultKID names the key in the JWKS and in every token header.
	DefaultKID = "e2e-key-1"
)

// Key is one signing key and its identifier.
type Key struct {
	KID  string
	priv *rsa.PrivateKey
}

// Generate makes a fresh 2048-bit RSA key.
func Generate(kid string) (*Key, error) {
	priv, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		return nil, err
	}
	return &Key{KID: kid, priv: priv}, nil
}

// WritePrivatePEM stores the private key (PKCS#8) with mode 0600.
func (k *Key) WritePrivatePEM(path string) error {
	der, err := x509.MarshalPKCS8PrivateKey(k.priv)
	if err != nil {
		return err
	}
	block := pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: der})
	return os.WriteFile(path, block, 0o600)
}

// LoadPrivatePEM reads a key written by WritePrivatePEM.
func LoadPrivatePEM(path, kid string) (*Key, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	block, _ := pem.Decode(raw)
	if block == nil {
		return nil, errors.New("idp: no PEM block in key file")
	}
	parsed, err := x509.ParsePKCS8PrivateKey(block.Bytes)
	if err != nil {
		return nil, err
	}
	priv, ok := parsed.(*rsa.PrivateKey)
	if !ok {
		return nil, errors.New("idp: key file is not an RSA key")
	}
	return &Key{KID: kid, priv: priv}, nil
}

// JWKS is the public key set in the shape authctx.loadJWKS reads: kty RSA,
// alg RS256, use sig, base64url modulus and exponent.
func (k *Key) JWKS() []byte {
	doc := map[string]any{"keys": []map[string]string{{
		"kid": k.KID,
		"kty": "RSA",
		"alg": "RS256",
		"use": "sig",
		"n":   base64.RawURLEncoding.EncodeToString(k.priv.N.Bytes()),
		"e":   base64.RawURLEncoding.EncodeToString(big.NewInt(int64(k.priv.E)).Bytes()),
	}}}
	out, _ := json.Marshal(doc)
	return out
}

// Handler serves the JWKS at /jwks.json and /.well-known/jwks.json.
func Handler(jwks []byte) http.Handler {
	mux := http.NewServeMux()
	serve := func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write(jwks)
	}
	mux.HandleFunc("/jwks.json", serve)
	mux.HandleFunc("/.well-known/jwks.json", serve)
	return mux
}

// Spec describes one token. The zero value of every optional field is a valid
// token for the broker; tests override one field to build a bad one.
type Spec struct {
	Issuer        string
	Audience      string
	OrgID         string
	PrincipalType string // "user" or "service"
	ActorID       string // user_id for a user, service_id for a service
	Scopes        []string
	ZDR           *bool
	// IssuedAt, NotBefore and Expires are absolute times. Zero values mean
	// "two minutes ago", "two minutes ago" and "two hours from now": the
	// margin keeps a container whose clock lags the host's from rejecting a
	// token whose iat is in its future.
	IssuedAt  time.Time
	NotBefore time.Time
	Expires   time.Time
}

// Person is a signed-in user of org.
func Person(issuer, org, user string) Spec {
	return Spec{Issuer: issuer, Audience: Audience, OrgID: org, PrincipalType: "user", ActorID: user}
}

// Service is a workload identity of org holding scopes.
func Service(issuer, org, id string, scopes ...string) Spec {
	return Spec{Issuer: issuer, Audience: Audience, OrgID: org, PrincipalType: "service", ActorID: id, Scopes: scopes}
}

// Sign issues the token.
func (k *Key) Sign(s Spec) (string, error) {
	now := time.Now()
	iat, nbf, exp := s.IssuedAt, s.NotBefore, s.Expires
	if iat.IsZero() {
		iat = now.Add(-2 * time.Minute)
	}
	if nbf.IsZero() {
		nbf = now.Add(-2 * time.Minute)
	}
	if exp.IsZero() {
		exp = now.Add(2 * time.Hour)
	}
	claims := jwt.MapClaims{
		"iss":            s.Issuer,
		"aud":            []string{s.Audience},
		"sub":            s.ActorID,
		"iat":            iat.Unix(),
		"nbf":            nbf.Unix(),
		"exp":            exp.Unix(),
		"org_id":         s.OrgID,
		"principal_type": s.PrincipalType,
	}
	switch s.PrincipalType {
	case "user":
		claims["user_id"] = s.ActorID
	case "service":
		claims["service_id"] = s.ActorID
	default:
		return "", fmt.Errorf("idp: unsupported principal type %q", s.PrincipalType)
	}
	if len(s.Scopes) > 0 {
		claims["scopes"] = s.Scopes
	}
	if s.ZDR != nil {
		claims["zdr"] = *s.ZDR
	}
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, claims)
	token.Header["kid"] = k.KID
	return token.SignedString(k.priv)
}
