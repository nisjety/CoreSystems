package actions

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"mime"
	"net/http"
	"net/url"
	"sort"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/triodelab/integration-corev2/internal/store"
)

// custom_rest.call (W4-CC-6, F-t3-057) is a server-side REST proxy for an organisation's own API.
// It exists so a model can call a customer REST endpoint without ever holding the API key:
//
//   - The connection (provider "custom_rest") names exactly one credential binding (W4-CC-3). The
//     binding fixes the host, the one header the secret goes in, and the methods and path prefixes
//     that may be forwarded. The request chooses none of those: it supplies a method, a path and a
//     query, and each must fall inside what the binding allows.
//   - https only, on the binding's host, port 443. Redirects are never followed (a 3xx is an
//     error). The call dials through the egress guard, so a host that resolves to a private,
//     loopback or metadata address is refused at connect time.
//   - 10 second deadline. The response is read up to 256 KiB; more than that is refused, never
//     truncated. The exact injected secret bytes are replaced with [redacted] in the response
//     before it is decoded, so an endpoint that echoes the credential cannot hand it back.
//   - The secret is placed only in the binding's header, never in the URL, query or body, and it is
//     never logged, audited or put in an error.
//
// GET and HEAD are reads (capability custom_rest.read); every other method is a write that needs
// the signed write attestation and an idempotency key like any other provider write
// (capability custom_rest.write, decided in internal/api/custom_rest.go). The operation is dark:
// while CUSTOM_REST_CONNECTORS_ENABLED is off the provider is "not implemented" exactly as before.
//
// Params:
//
//	method   required, one of the binding's allowed methods
//	path     required, absolute, under one of the binding's allowed path prefixes
//	query    optional object of scalar values (or arrays of scalars)
//
// Body: optional JSON object, sent only with POST, PUT, PATCH or DELETE.
const (
	// CustomRESTProviderKey is the provider key of a connection served by custom_rest.call.
	CustomRESTProviderKey = "custom_rest"
	// OperationCustomRESTCall is the one operation of the custom_rest provider.
	OperationCustomRESTCall = "custom_rest.call"
	// CustomRESTCapabilityRead is the capability a GET or HEAD call needs.
	CustomRESTCapabilityRead = "custom_rest.read"
	// CustomRESTCapabilityWrite is the capability every other method needs.
	CustomRESTCapabilityWrite = "custom_rest.write"
	// CustomRESTBindingContextKey is the connection providerContext key that names the one
	// credential binding the connection calls with.
	CustomRESTBindingContextKey = "credential_binding_id"

	customRESTTimeout          = 10 * time.Second
	customRESTMaxResponseBytes = 256 << 10
	customRESTMaxRequestBytes  = 256 << 10
	customRESTMaxPathLength    = 2048
	customRESTMaxQueryEntries  = 32
	customRESTMaxQueryKeyLen   = 128
	customRESTMaxQueryValueLen = 2048
	customRESTRedacted         = "[redacted]"
)

// customRESTMethods is the closed list of methods a call may use. A binding narrows it further.
var customRESTMethods = []string{"GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"}

// CustomRESTBinding is the credential binding a custom_rest.call runs with, opened by the HTTP
// layer for exactly one call. Secret is the plaintext credential: it is held for the duration of
// the request and is never serialised, logged or returned.
type CustomRESTBinding struct {
	ID                  string
	Kind                string
	Host                string
	HeaderName          string
	AllowedMethods      []string
	AllowedPathPrefixes []string
	Secret              string
}

// String and GoString omit the secret so a stray %v or %#v cannot print it.
func (b CustomRESTBinding) String() string   { return "actions.CustomRESTBinding{" + b.ID + "}" }
func (b CustomRESTBinding) GoString() string { return b.String() }

// IsCustomRESTOperation reports whether operation is the custom_rest call, in any letter case.
func IsCustomRESTOperation(operation string) bool {
	return strings.ToLower(strings.TrimSpace(operation)) == OperationCustomRESTCall
}

// CustomRESTMethod returns the normalised (trimmed, upper-case) method of a custom_rest call's
// params, or "" when it is absent or not a string.
func CustomRESTMethod(params map[string]any) string {
	method, _ := params["method"].(string)
	return strings.ToUpper(strings.TrimSpace(method))
}

// CustomRESTIsRead reports whether a call is a read: GET or HEAD. Anything else, including a
// missing or unknown method, is a write, so a malformed request can never lower the bar.
func CustomRESTIsRead(params map[string]any) bool {
	method := CustomRESTMethod(params)
	return method == http.MethodGet || method == http.MethodHead
}

func (s *Service) executeCustomREST(ctx context.Context, binding *CustomRESTBinding, operation string, params, body map[string]any) (any, error) {
	if !IsCustomRESTOperation(operation) {
		return nil, fmt.Errorf("unsupported custom_rest operation %q", operation)
	}
	if binding == nil {
		return nil, errors.New("custom_rest call has no credential binding")
	}
	header, headerValue, err := binding.injection()
	if err != nil {
		return nil, err
	}

	method := CustomRESTMethod(params)
	if !customRESTMethodKnown(method) {
		return nil, errors.New("method must be one of GET, HEAD, POST, PUT, PATCH, DELETE or OPTIONS")
	}
	if !customRESTMethodAllowed(binding.AllowedMethods, method) {
		return nil, fmt.Errorf("method %s is not allowed by the credential binding", method)
	}
	path, _ := params["path"].(string)
	path = strings.TrimSpace(path)
	if !customRESTPathValid(path) {
		return nil, errors.New("path must be an absolute path of plain URL path characters with no query, fragment, encoding, empty or dot segments")
	}
	if !customRESTPathAllowed(binding.AllowedPathPrefixes, path) {
		return nil, errors.New("path is not under a path prefix allowed by the credential binding")
	}
	rawQuery, err := customRESTQuery(params["query"])
	if err != nil {
		return nil, err
	}
	var payload io.Reader
	if len(body) > 0 {
		if method == http.MethodGet || method == http.MethodHead || method == http.MethodOptions {
			return nil, fmt.Errorf("a %s call cannot carry a body", method)
		}
		encoded, marshalErr := json.Marshal(body)
		if marshalErr != nil {
			return nil, errors.New("body must be a JSON object")
		}
		if len(encoded) > customRESTMaxRequestBytes {
			return nil, errors.New("body exceeds the 256 KiB limit")
		}
		payload = bytes.NewReader(encoded)
	}

	// The host is the binding's, never the request's. The scheme is fixed to https.
	target := url.URL{Scheme: "https", Host: binding.Host, Path: path, RawQuery: rawQuery}
	callCtx, cancel := context.WithTimeout(ctx, customRESTTimeout)
	defer cancel()
	req, err := http.NewRequestWithContext(callCtx, method, target.String(), payload)
	if err != nil {
		return nil, errors.New("custom_rest request could not be built")
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", "verevon-integration/1")
	if payload != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	// The one place the secret enters the request.
	req.Header.Set(header, headerValue)

	resp, err := customRESTClient(s.httpClient).Do(req)
	if err != nil {
		return nil, binding.scrubError(err)
	}
	defer resp.Body.Close()

	if resp.StatusCode >= 300 && resp.StatusCode < 400 {
		return nil, errors.New("custom_rest endpoint answered with a redirect; redirects are not followed")
	}
	if resp.ContentLength > customRESTMaxResponseBytes {
		return nil, errors.New("custom_rest response exceeds the 256 KiB limit")
	}
	data, err := io.ReadAll(io.LimitReader(resp.Body, customRESTMaxResponseBytes+1))
	if err != nil {
		return nil, binding.scrubError(err)
	}
	if len(data) > customRESTMaxResponseBytes {
		return nil, errors.New("custom_rest response exceeds the 256 KiB limit")
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		// The status is the only thing a failed call returns: an error body can carry request
		// content or the credential.
		return nil, fmt.Errorf("custom_rest endpoint returned status %d", resp.StatusCode)
	}

	data = redactCustomRESTSecret(data, binding.Secret)
	contentType := strings.TrimSpace(resp.Header.Get("Content-Type"))
	return map[string]any{
		"status":       resp.StatusCode,
		"content_type": contentType,
		"body":         decodeCustomRESTBody(contentType, data),
	}, nil
}

// injection validates the binding and returns the header name and value the secret goes in. A
// bearer kind sends "Bearer <secret>"; an API-key kind sends the secret as is.
func (b *CustomRESTBinding) injection() (string, string, error) {
	switch b.Kind {
	case store.CredentialBindingKindRESTAPIKey, store.CredentialBindingKindRESTBearer:
	default:
		return "", "", errors.New("the credential binding is not a REST credential")
	}
	if !customRESTHostValid(b.Host) {
		return "", "", errors.New("the credential binding host is not valid")
	}
	switch b.HeaderName {
	case "Authorization", "X-Api-Key", "Api-Key", "X-Auth-Token":
	default:
		return "", "", errors.New("the credential binding header is not valid")
	}
	if b.Kind == store.CredentialBindingKindRESTBearer && b.HeaderName != "Authorization" {
		return "", "", errors.New("the credential binding header is not valid")
	}
	if !customRESTSecretValid(b.Secret) {
		return "", "", errors.New("the credential is not available")
	}
	if b.Kind == store.CredentialBindingKindRESTBearer {
		return b.HeaderName, "Bearer " + b.Secret, nil
	}
	return b.HeaderName, b.Secret, nil
}

// scrubError reports a transport failure without the request URL (url.Error carries it) and
// without the credential, keeping the cause chain so a caller can still test for a blocked host.
func (b *CustomRESTBinding) scrubError(err error) error {
	var urlErr *url.Error
	if errors.As(err, &urlErr) && urlErr.Err != nil {
		err = urlErr.Err
	}
	if b.Secret != "" && strings.Contains(err.Error(), b.Secret) {
		return errors.New("call custom_rest endpoint failed")
	}
	return fmt.Errorf("call custom_rest endpoint: %w", err)
}

// customRESTClient returns a copy of base that never follows a redirect, whatever the injected
// client was configured to do. The Transport (and with it the egress guard's dialer) is shared.
func customRESTClient(base *http.Client) *http.Client {
	client := *base
	client.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	return &client
}

func customRESTMethodKnown(method string) bool {
	for _, known := range customRESTMethods {
		if method == known {
			return true
		}
	}
	return false
}

func customRESTMethodAllowed(allowed []string, method string) bool {
	for _, candidate := range allowed {
		if strings.ToUpper(strings.TrimSpace(candidate)) == method {
			return true
		}
	}
	return false
}

// customRESTPathAllowed reports whether path is one of the binding's prefixes or sits below one at
// a segment boundary: "/v1/items" admits "/v1/items" and "/v1/items/7", never "/v1/itemsX".
func customRESTPathAllowed(prefixes []string, path string) bool {
	for _, prefix := range prefixes {
		prefix = strings.TrimSpace(prefix)
		if prefix == "" || prefix[0] != '/' {
			continue
		}
		if path == prefix || strings.HasPrefix(path, strings.TrimSuffix(prefix, "/")+"/") {
			return true
		}
	}
	return false
}

// customRESTPathValid admits only plain URL path characters, so nothing in the path can change the
// host, start a query or fragment, smuggle an encoded dot segment or switch to another route.
func customRESTPathValid(path string) bool {
	if path == "" || len(path) > customRESTMaxPathLength || path[0] != '/' || strings.Contains(path, "//") {
		return false
	}
	for index := 0; index < len(path); index++ {
		if !customRESTPathByte(path[index]) {
			return false
		}
	}
	for _, segment := range strings.Split(path, "/") {
		if segment == "." || segment == ".." {
			return false
		}
	}
	return true
}

func customRESTPathByte(ch byte) bool {
	switch {
	case ch >= 'a' && ch <= 'z', ch >= 'A' && ch <= 'Z', ch >= '0' && ch <= '9':
		return true
	}
	return strings.IndexByte("/-._~:@,+=!$&'()*", ch) >= 0
}

// customRESTHostValid re-checks the binding host so a corrupted row can never widen the target: a
// lowercase DNS name only, with no port, userinfo, path or IP literal.
func customRESTHostValid(host string) bool {
	if host == "" || len(host) > 253 || host != strings.ToLower(host) || !strings.Contains(host, ".") {
		return false
	}
	for index := 0; index < len(host); index++ {
		ch := host[index]
		if !(ch >= 'a' && ch <= 'z') && !(ch >= '0' && ch <= '9') && ch != '-' && ch != '.' {
			return false
		}
	}
	if strings.HasPrefix(host, ".") || strings.HasSuffix(host, ".") || strings.Contains(host, "..") {
		return false
	}
	last := host[strings.LastIndexByte(host, '.')+1:]
	return strings.Trim(last, "0123456789") != ""
}

// customRESTSecretValid admits the shape a binding secret is stored in: visible ASCII with no
// spaces, so it can never break out of a header value.
func customRESTSecretValid(secret string) bool {
	if secret == "" || len(secret) > 4096 {
		return false
	}
	for index := 0; index < len(secret); index++ {
		if secret[index] <= 0x20 || secret[index] >= 0x7f {
			return false
		}
	}
	return true
}

// customRESTQuery encodes the query object. Only scalars (or arrays of scalars) are accepted; the
// keys are sorted so the same params always give the same URL.
func customRESTQuery(raw any) (string, error) {
	if raw == nil {
		return "", nil
	}
	query, ok := raw.(map[string]any)
	if !ok {
		return "", errors.New("query must be an object")
	}
	if len(query) > customRESTMaxQueryEntries {
		return "", errors.New("query has too many entries")
	}
	keys := make([]string, 0, len(query))
	for key := range query {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	values := url.Values{}
	for _, key := range keys {
		name := strings.TrimSpace(key)
		if name == "" || len(name) > customRESTMaxQueryKeyLen {
			return "", errors.New("query keys must be 1 to 128 characters")
		}
		var items []any
		switch typed := query[key].(type) {
		case []any:
			items = typed
		default:
			items = []any{typed}
		}
		for _, item := range items {
			text, ok := customRESTQueryScalar(item)
			if !ok || len(text) > customRESTMaxQueryValueLen {
				return "", errors.New("query values must be strings, numbers or booleans of at most 2048 characters")
			}
			values.Add(name, text)
		}
	}
	return values.Encode(), nil
}

func customRESTQueryScalar(value any) (string, bool) {
	switch typed := value.(type) {
	case string:
		return typed, utf8.ValidString(typed)
	case bool:
		return strconv.FormatBool(typed), true
	case float64:
		return strconv.FormatFloat(typed, 'f', -1, 64), true
	case int:
		return strconv.Itoa(typed), true
	case int64:
		return strconv.FormatInt(typed, 10), true
	case json.Number:
		return typed.String(), true
	}
	return "", false
}

// redactCustomRESTSecret replaces the exact injected secret bytes with [redacted], in the raw
// form and in the escaped forms an endpoint would echo it in (JSON string, URL query). The whole
// response is in memory, so a match cannot straddle a chunk boundary.
func redactCustomRESTSecret(data []byte, secret string) []byte {
	if secret == "" {
		return data
	}
	forms := []string{secret}
	if encoded, err := json.Marshal(secret); err == nil && len(encoded) >= 2 {
		if inner := string(encoded[1 : len(encoded)-1]); inner != secret {
			forms = append(forms, inner)
		}
	}
	if escaped := url.QueryEscape(secret); escaped != secret {
		forms = append(forms, escaped)
	}
	for _, form := range forms {
		data = bytes.ReplaceAll(data, []byte(form), []byte(customRESTRedacted))
	}
	return data
}

// decodeCustomRESTBody returns the response body as JSON when it is JSON, otherwise as text.
func decodeCustomRESTBody(contentType string, data []byte) any {
	trimmed := bytes.TrimSpace(data)
	if len(trimmed) == 0 {
		return nil
	}
	if mediaType, _, err := mime.ParseMediaType(contentType); err == nil &&
		(mediaType == "application/json" || strings.HasSuffix(mediaType, "+json")) {
		var decoded any
		if json.Unmarshal(trimmed, &decoded) == nil {
			return decoded
		}
	}
	return strings.ToValidUTF8(string(data), "�")
}
