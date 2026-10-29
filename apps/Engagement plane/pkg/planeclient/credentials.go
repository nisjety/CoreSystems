package planeclient

import (
	"encoding/base64"
	"encoding/json"
	"io"
	"os"
	"strings"
	"syscall"
	"time"
)

// bearer is a local deployment-binding assertion, never cryptographic validation.
// Files are reopened on every operation so short-lived credentials can rotate.
func (c *Client) bearer(path, audience, scope string) (string, error) {
	denied := func() (string, error) { return "", unavailable(audience, "missing_credential") }
	if path == "" {
		return denied()
	}
	fd, err := syscall.Open(path, syscall.O_RDONLY|syscall.O_NOFOLLOW|syscall.O_NONBLOCK|syscall.O_CLOEXEC, 0)
	if err != nil {
		return denied()
	}
	f := os.NewFile(uintptr(fd), "plane-bearer")
	defer f.Close()
	info, err := f.Stat()
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm()&0077 != 0 || info.Size() > 8192 {
		return denied()
	}
	raw, err := io.ReadAll(io.LimitReader(f, 8193))
	if err != nil || len(raw) > 8192 {
		return denied()
	}
	token := strings.TrimSpace(string(raw))
	parts := strings.Split(token, ".")
	if len(parts) != 3 || strings.ContainsAny(token, " \t\r\n") || parts[2] == "" {
		return denied()
	}
	header, err := base64.RawURLEncoding.DecodeString(parts[0])
	if err != nil {
		return denied()
	}
	var h struct {
		Algorithm string `json:"alg"`
	}
	if json.Unmarshal(header, &h) != nil || h.Algorithm != "RS256" {
		return denied()
	}
	if _, err = base64.RawURLEncoding.DecodeString(parts[2]); err != nil {
		return denied()
	}
	body, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		return denied()
	}
	var claims struct {
		Org       string   `json:"org_id"`
		Audience  string   `json:"aud"`
		Subject   string   `json:"sub"`
		Service   string   `json:"service_id"`
		Principal string   `json:"principal_type"`
		User      string   `json:"user_id"`
		Expires   int64    `json:"exp"`
		NotBefore int64    `json:"nbf"`
		Scopes    []string `json:"scopes"`
	}
	if json.Unmarshal(body, &claims) != nil || claims.Org != c.cfg.OrganizationID || claims.Audience != audience || claims.Principal != "service" || claims.Service != "service:"+c.cfg.Core || claims.Subject != claims.Service || claims.User != "" || claims.Expires <= time.Now().Add(15*time.Second).Unix() || claims.NotBefore > time.Now().Unix() {
		return denied()
	}
	for _, s := range claims.Scopes {
		if s == scope {
			return token, nil
		}
	}
	return denied()
}
func (c *Client) modelBearers() (string, string, error) {
	if c == nil || c.cfg.ModelURL == "" {
		return "", "", unavailable("model", "not_configured")
	}
	if c.cfg.ModelTokenFile == c.cfg.InferenceTokenFile {
		return "", "", unavailable("model", "missing_credential")
	}
	gateway, err := c.bearer(c.cfg.ModelTokenFile, "model-gateway", "models:invoke")
	if err != nil {
		return "", "", unavailable("model", "missing_credential")
	}
	inference, err := c.bearer(c.cfg.InferenceTokenFile, "inference-core", "inference:invoke")
	if err != nil || inference == gateway {
		return "", "", unavailable("model", "missing_credential")
	}
	return gateway, inference, nil
}
