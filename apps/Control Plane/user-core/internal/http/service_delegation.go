package http

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"io"
	"net/http"
	"strings"
	"time"
)

const (
	serviceDelegationMaxAge     = 30 * time.Second
	serviceDelegationFutureSkew = 5 * time.Second
	serviceDelegationMaxBody    = 1 << 20
)

type serviceDelegationClaims struct {
	Version      string
	Principal    string
	Audience     string
	Timestamp    string
	Method       string
	URI          string
	UserID       string
	OrgID        string
	Email        string
	Name         string
	Avatar       string
	BodySHA256   string
	Operation    string
	ResourceType string
	ResourceID   string
	Reason       string
	ZDR          string
	Nonce        string
}

type serviceDelegationV2Claims struct {
	Principal    string
	Audience     string
	Timestamp    string
	Method       string
	URI          string
	UserID       string
	OrgID        string
	Operation    string
	ResourceType string
	ResourceID   string
	Reason       string
	ZDR          string
	Nonce        string
	BodySHA256   string
}

func serviceDelegationBodyDigest(body []byte) string {
	digest := sha256.Sum256(body)
	return base64.RawURLEncoding.EncodeToString(digest[:])
}

func serviceDelegationSignature(token string, claims serviceDelegationClaims) string {
	canonical := strings.Join([]string{
		"v1",
		claims.Principal,
		claims.Audience,
		claims.Timestamp,
		claims.Method,
		claims.URI,
		claims.UserID,
		claims.OrgID,
		claims.Email,
		claims.Name,
		claims.Avatar,
		claims.BodySHA256,
	}, "\n")
	mac := hmac.New(sha256.New, []byte(token))
	_, _ = mac.Write([]byte(canonical))
	return base64.RawURLEncoding.EncodeToString(mac.Sum(nil))
}

func serviceDelegationSignatureV2(token string, claims serviceDelegationV2Claims) string {
	canonical := strings.Join([]string{
		"v2",
		claims.Principal,
		claims.Audience,
		claims.Timestamp,
		claims.Method,
		claims.URI,
		claims.UserID,
		claims.OrgID,
		claims.Operation,
		claims.ResourceType,
		claims.ResourceID,
		claims.Reason,
		claims.ZDR,
		claims.Nonce,
		claims.BodySHA256,
	}, "\n")
	mac := hmac.New(sha256.New, []byte(token))
	_, _ = mac.Write([]byte(canonical))
	return base64.RawURLEncoding.EncodeToString(mac.Sum(nil))
}

func delegationRequestBody(request *http.Request) ([]byte, bool) {
	if request.Body == nil || request.Body == http.NoBody {
		return nil, true
	}
	body, err := io.ReadAll(io.LimitReader(request.Body, serviceDelegationMaxBody+1))
	if err != nil || len(body) > serviceDelegationMaxBody {
		return nil, false
	}
	request.Body = io.NopCloser(strings.NewReader(string(body)))
	return body, true
}

func verifyServiceDelegation(request *http.Request, credential serviceCredential, now time.Time) (serviceDelegationClaims, bool) {
	timestampValue := strings.TrimSpace(request.Header.Get("X-Delegation-Timestamp"))
	timestamp, err := time.Parse(time.RFC3339, timestampValue)
	if err != nil {
		return serviceDelegationClaims{}, false
	}
	age := now.UTC().Sub(timestamp.UTC())
	if age > serviceDelegationMaxAge || age < -serviceDelegationFutureSkew {
		return serviceDelegationClaims{}, false
	}

	body, ok := delegationRequestBody(request)
	if !ok {
		return serviceDelegationClaims{}, false
	}
	bodyDigest := serviceDelegationBodyDigest(body)
	providedBodyDigest := strings.TrimSpace(request.Header.Get("X-Delegation-Body-SHA256"))
	if !secureServiceTokenEqual(bodyDigest, providedBodyDigest) {
		return serviceDelegationClaims{}, false
	}

	if strings.TrimSpace(request.Header.Get("X-Delegation-Version")) == "v2" {
		claims := serviceDelegationV2Claims{
			Principal:    credential.Principal,
			Audience:     credential.Audience,
			Timestamp:    timestampValue,
			Method:       request.Method,
			URI:          request.URL.RequestURI(),
			UserID:       strings.TrimSpace(request.Header.Get("X-User-Id")),
			OrgID:        strings.TrimSpace(request.Header.Get("X-Org-Id")),
			Operation:    strings.TrimSpace(request.Header.Get("X-Delegation-Operation")),
			ResourceType: strings.TrimSpace(request.Header.Get("X-Delegation-Resource-Type")),
			ResourceID:   strings.TrimSpace(request.Header.Get("X-Delegation-Resource-Id")),
			Reason:       strings.TrimSpace(request.Header.Get("X-Delegation-Reason")),
			ZDR:          strings.TrimSpace(request.Header.Get("X-Delegation-ZDR")),
			Nonce:        strings.TrimSpace(request.Header.Get("X-Delegation-Nonce")),
			BodySHA256:   bodyDigest,
		}
		if claims.UserID == "" || claims.OrgID == "" || claims.Operation == "" || claims.ResourceType == "" || len(claims.Reason) < 3 || len(claims.Reason) > 500 || claims.ZDR != "true" || len(claims.Nonce) < 16 || len(claims.Nonce) > 128 {
			return serviceDelegationClaims{}, false
		}
		providedSignature, err := base64.RawURLEncoding.DecodeString(strings.TrimSpace(request.Header.Get("X-Delegation-Signature")))
		if err != nil {
			return serviceDelegationClaims{}, false
		}
		expectedSignature, err := base64.RawURLEncoding.DecodeString(serviceDelegationSignatureV2(credential.Token, claims))
		if err != nil || !hmac.Equal(expectedSignature, providedSignature) {
			return serviceDelegationClaims{}, false
		}
		return serviceDelegationClaims{
			Version: "v2", Principal: claims.Principal, Audience: claims.Audience,
			Timestamp: claims.Timestamp, Method: claims.Method, URI: claims.URI,
			UserID: claims.UserID, OrgID: claims.OrgID, BodySHA256: claims.BodySHA256,
			Operation: claims.Operation, ResourceType: claims.ResourceType,
			ResourceID: claims.ResourceID, Reason: claims.Reason, ZDR: claims.ZDR,
			Nonce: claims.Nonce,
		}, true
	}

	claims := serviceDelegationClaims{
		Version:    "v1",
		Principal:  credential.Principal,
		Audience:   credential.Audience,
		Timestamp:  timestampValue,
		Method:     request.Method,
		URI:        request.URL.RequestURI(),
		UserID:     strings.TrimSpace(request.Header.Get("X-User-Id")),
		OrgID:      strings.TrimSpace(request.Header.Get("X-Org-Id")),
		Email:      strings.ToLower(strings.TrimSpace(request.Header.Get("X-User-Email"))),
		Name:       strings.TrimSpace(request.Header.Get("X-User-Name")),
		Avatar:     strings.TrimSpace(request.Header.Get("X-User-Avatar")),
		BodySHA256: bodyDigest,
	}
	if claims.UserID == "" {
		return serviceDelegationClaims{}, false
	}

	providedSignature, err := base64.RawURLEncoding.DecodeString(strings.TrimSpace(request.Header.Get("X-Delegation-Signature")))
	if err != nil {
		return serviceDelegationClaims{}, false
	}
	expectedSignature, err := base64.RawURLEncoding.DecodeString(serviceDelegationSignature(credential.Token, claims))
	if err != nil || !hmac.Equal(expectedSignature, providedSignature) {
		return serviceDelegationClaims{}, false
	}
	return claims, true
}
