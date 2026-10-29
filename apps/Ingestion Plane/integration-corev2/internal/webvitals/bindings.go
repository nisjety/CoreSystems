package webvitals

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"io"
	"net/url"
	"os"
	"path/filepath"
	"strings"

	"github.com/triodelab/integration-corev2/internal/pagespeed"
)

var errBinding = errors.New("web vitals requires protected valid deployment bindings and token files")

type binding struct {
	OrganizationID           string   `json:"organizationId"`
	WebsiteID                string   `json:"websiteId"`
	WebsiteRevision          int64    `json:"websiteRevision"`
	Site                     string   `json:"site"`
	Environment              string   `json:"environment"`
	ReportingOrigin          string   `json:"reportingOrigin"`
	DevelopmentBackendOrigin string   `json:"developmentBackendOrigin,omitempty"`
	TokenFile                string   `json:"tokenFile"`
	Paths                    []string `json:"paths"`
	epoch                    [32]byte
}

func bindingKey(org string, in Input) string {
	return org + "\n" + in.WebsiteID + "\n" + in.Environment
}
func protectedFile(path string, max int64) ([]byte, error) {
	if !filepath.IsAbs(path) || filepath.Clean(path) != path {
		return nil, errBinding
	}
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm() != 0600 || info.Size() < 1 || info.Size() > max {
		return nil, errBinding
	}
	f, err := os.Open(path)
	if err != nil {
		return nil, errBinding
	}
	defer f.Close()
	current, err := f.Stat()
	if err != nil || !os.SameFile(info, current) || !current.Mode().IsRegular() || current.Mode().Perm() != 0600 {
		return nil, errBinding
	}
	raw, err := io.ReadAll(io.LimitReader(f, max+1))
	if err != nil || int64(len(raw)) > max {
		return nil, errBinding
	}
	return raw, nil
}
func readToken(path string) (string, error) {
	raw, err := protectedFile(path, 512)
	if err != nil {
		return "", errBinding
	}
	token := strings.TrimSpace(string(raw))
	if len(token) < 32 || len(token) > 256 {
		return "", errBinding
	}
	for _, c := range token {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '_' || c == '-') {
			return "", errBinding
		}
	}
	return token, nil
}
func loadBindings(path, runtime string) (map[string]binding, error) {
	result := map[string]binding{}
	if path == "" {
		return result, nil
	}
	raw, err := protectedFile(path, 64<<10)
	if err != nil || UniqueJSON(raw) != nil || !exactKeys(raw, []string{"version", "installations"}, nil) {
		return nil, errBinding
	}
	var shape struct {
		Installations []json.RawMessage `json:"installations"`
	}
	if json.Unmarshal(raw, &shape) != nil {
		return nil, errBinding
	}
	for _, installation := range shape.Installations {
		if !exactKeys(installation, []string{"organizationId", "websiteId", "websiteRevision", "site", "environment", "reportingOrigin", "tokenFile", "paths"}, []string{"developmentBackendOrigin"}) {
			return nil, errBinding
		}
	}
	var manifest struct {
		Version       int       `json:"version"`
		Installations []binding `json:"installations"`
	}
	d := json.NewDecoder(bytes.NewReader(raw))
	d.DisallowUnknownFields()
	if d.Decode(&manifest) != nil || manifest.Version != 1 || len(manifest.Installations) > 32 {
		return nil, errBinding
	}
	credentialHashes := map[[32]byte]bool{}
	for _, item := range manifest.Installations {
		if !orgPattern.MatchString(item.OrganizationID) || !validInput(Input{item.WebsiteID, item.Site, item.WebsiteRevision, item.Environment}) || !validOrigin(item, runtime) || len(item.Paths) < 1 || len(item.Paths) > 64 {
			return nil, errBinding
		}
		seen := map[string]bool{}
		for _, p := range item.Paths {
			if !validPath(p) || seen[p] {
				return nil, errBinding
			}
			seen[p] = true
		}
		key := bindingKey(item.OrganizationID, Input{WebsiteID: item.WebsiteID, Environment: item.Environment})
		if _, ok := result[key]; ok {
			return nil, errBinding
		}
		token, err := readToken(item.TokenFile)
		if err != nil {
			return nil, errBinding
		}
		item.epoch = sha256.Sum256([]byte(token))
		if credentialHashes[item.epoch] {
			return nil, errBinding
		}
		credentialHashes[item.epoch] = true
		result[key] = item
	}
	return result, nil
}

func exactKeys(raw []byte, required, optional []string) bool {
	var values map[string]json.RawMessage
	if json.Unmarshal(raw, &values) != nil {
		return false
	}
	allowed := map[string]bool{}
	for _, name := range required {
		if _, ok := values[name]; !ok {
			return false
		}
		allowed[name] = true
	}
	for _, name := range optional {
		allowed[name] = true
	}
	for name := range values {
		if !allowed[name] {
			return false
		}
	}
	return true
}
func validInput(in Input) bool {
	return idPattern.MatchString(in.WebsiteID) && in.WebsiteRevision > 0 && in.WebsiteRevision <= 9007199254740991 && (in.Environment == "production" || in.Environment == "development") && pagespeed.ValidateInput(in.Site, "mobile") == nil
}
func validOrigin(b binding, runtime string) bool {
	if b.Environment == "development" {
		return (runtime == "development" || runtime == "local") && ((b.ReportingOrigin == "http://localhost:3000" && (b.DevelopmentBackendOrigin == "" || b.DevelopmentBackendOrigin == "http://host.docker.internal:3000")) || (b.ReportingOrigin == "http://localhost:3001" && (b.DevelopmentBackendOrigin == "" || b.DevelopmentBackendOrigin == "http://host.docker.internal:3001")))
	}
	if b.DevelopmentBackendOrigin != "" {
		return false
	}
	u, err := url.Parse(b.ReportingOrigin)
	if err != nil || u.Scheme != "https" || u.User != nil || u.RawQuery != "" || u.ForceQuery || u.Fragment != "" || u.RawPath != "" || u.Path != "" || u.Port() != "" || u.String() != b.ReportingOrigin {
		return false
	}
	site, _ := url.Parse(b.Site)
	host := site.Hostname()
	return u.Hostname() == host || u.Hostname() == "www."+strings.TrimPrefix(host, "www.") || "www."+u.Hostname() == host
}
func validPath(p string) bool {
	if len(p) > 160 || !pathPattern.MatchString(p) {
		return false
	}
	for _, part := range strings.Split(p, "/") {
		switch part {
		case "account", "admin", "api", "auth", "login", "logout", "signin", "signup", "register", "reset-password", "checkout", "payment", "kontakt", "contact", "personvern", "cookies", "vilkar", "salgsvilkar":
			return false
		}
	}
	return true
}

// UniqueJSON rejects duplicate object keys and deeply nested payloads before
// decoding structs. Unknown export fields are discarded, never forwarded.
func UniqueJSON(raw []byte) error {
	d := json.NewDecoder(bytes.NewReader(raw))
	d.UseNumber()
	if err := jsonValue(d, 0); err != nil {
		return ErrInvalid
	}
	if _, err := d.Token(); err != io.EOF {
		return ErrInvalid
	}
	return nil
}
func jsonValue(d *json.Decoder, depth int) error {
	if depth > 16 {
		return ErrInvalid
	}
	token, err := d.Token()
	if err != nil {
		return ErrInvalid
	}
	delim, ok := token.(json.Delim)
	if !ok {
		return nil
	}
	switch delim {
	case '{':
		seen := map[string]bool{}
		for d.More() {
			k, err := d.Token()
			name, ok := k.(string)
			if err != nil || !ok || seen[name] {
				return ErrInvalid
			}
			seen[name] = true
			if jsonValue(d, depth+1) != nil {
				return ErrInvalid
			}
		}
		end, err := d.Token()
		if err != nil || end != json.Delim('}') {
			return ErrInvalid
		}
	case '[':
		for d.More() {
			if jsonValue(d, depth+1) != nil {
				return ErrInvalid
			}
		}
		end, err := d.Token()
		if err != nil || end != json.Delim(']') {
			return ErrInvalid
		}
	default:
		return ErrInvalid
	}
	return nil
}
