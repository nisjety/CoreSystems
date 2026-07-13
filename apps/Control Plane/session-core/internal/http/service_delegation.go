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
	sessionDelegationMaxAge     = 30 * time.Second
	sessionDelegationFutureSkew = 5 * time.Second
	sessionDelegationMaxBody    = 1 << 20
)

type sessionDelegationClaims struct {
	Principal  string
	Audience   string
	Timestamp  string
	Method     string
	URI        string
	UserID     string
	OrgID      string
	Email      string
	Name       string
	Avatar     string
	BodySHA256 string
}

func sessionDelegationBodyDigest(body []byte) string {
	digest := sha256.Sum256(body)
	return base64.RawURLEncoding.EncodeToString(digest[:])
}

func sessionDelegationSignature(token string, claims sessionDelegationClaims) string {
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

func verifySessionServiceDelegation(request *http.Request, credential serviceCredential, now time.Time) (sessionDelegationClaims, bool) {
	timestampValue := strings.TrimSpace(request.Header.Get("X-Delegation-Timestamp"))
	timestamp, err := time.Parse(time.RFC3339, timestampValue)
	if err != nil {
		return sessionDelegationClaims{}, false
	}
	age := now.UTC().Sub(timestamp.UTC())
	if age > sessionDelegationMaxAge || age < -sessionDelegationFutureSkew {
		return sessionDelegationClaims{}, false
	}

	body, ok := readSessionDelegationBody(request)
	if !ok {
		return sessionDelegationClaims{}, false
	}
	bodyDigest := sessionDelegationBodyDigest(body)
	if !constantTimeEncodedEqual(bodyDigest, request.Header.Get("X-Delegation-Body-SHA256")) {
		return sessionDelegationClaims{}, false
	}

	claims := sessionDelegationClaims{
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
		return sessionDelegationClaims{}, false
	}
	if !constantTimeEncodedEqual(sessionDelegationSignature(credential.Token, claims), request.Header.Get("X-Delegation-Signature")) {
		return sessionDelegationClaims{}, false
	}
	return claims, true
}

func readSessionDelegationBody(request *http.Request) ([]byte, bool) {
	if request.Body == nil || request.Body == http.NoBody {
		return nil, true
	}
	body, err := io.ReadAll(io.LimitReader(request.Body, sessionDelegationMaxBody+1))
	if err != nil || len(body) > sessionDelegationMaxBody {
		return nil, false
	}
	request.Body = io.NopCloser(strings.NewReader(string(body)))
	return body, true
}

func constantTimeEncodedEqual(expected, provided string) bool {
	expectedBytes, expectedErr := base64.RawURLEncoding.DecodeString(strings.TrimSpace(expected))
	providedBytes, providedErr := base64.RawURLEncoding.DecodeString(strings.TrimSpace(provided))
	return expectedErr == nil && providedErr == nil && hmac.Equal(expectedBytes, providedBytes)
}
