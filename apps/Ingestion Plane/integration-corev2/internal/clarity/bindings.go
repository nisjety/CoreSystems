// Package clarity owns fixed-endpoint, read-only project export reports.
// Credentials and operator-confirmed organization/site bindings never cross
// the public API boundary. A binding is not proof of domain ownership.
package clarity

import (
	"crypto/sha256"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/triodelab/integration-corev2/internal/pagespeed"
)

type binding struct {
	OrganizationID  string `json:"organizationId"`
	Site            string `json:"site"`
	ProjectID       string `json:"projectId"`
	TokenFile       string `json:"tokenFile"`
	ConfirmedAt     string `json:"confirmedAt"`
	credentialEpoch [32]byte
}

var errBinding = errors.New("Clarity bindings must use a protected valid deployment manifest and token files")

func loadBindings(path string) (map[string]binding, error) {
	bindings := map[string]binding{}
	if path == "" {
		return bindings, nil
	}
	if !filepath.IsAbs(path) {
		return nil, errBinding
	}
	data, err := protectedFile(path, 32<<10)
	if err != nil {
		return nil, errBinding
	}
	var manifest struct {
		Version  int       `json:"version"`
		Bindings []binding `json:"bindings"`
	}
	decoder := json.NewDecoder(strings.NewReader(string(data)))
	decoder.DisallowUnknownFields()
	if decoder.Decode(&manifest) != nil || manifest.Version != 1 || len(manifest.Bindings) > 32 {
		return nil, errBinding
	}
	var trailing any
	if decoder.Decode(&trailing) != io.EOF {
		return nil, errBinding
	}
	projects := map[string]bool{}
	for _, item := range manifest.Bindings {
		if !safeID(item.OrganizationID, 128) || !safeID(item.ProjectID, 32) || pagespeed.ValidateInput(item.Site, "mobile") != nil || !safeID(item.TokenFile, 96) || item.TokenFile == "." || item.TokenFile == ".." {
			return nil, errBinding
		}
		if _, err := time.Parse(time.RFC3339, item.ConfirmedAt); err != nil {
			return nil, errBinding
		}
		key := item.OrganizationID + "\n" + item.Site
		project := item.OrganizationID + "\n" + item.ProjectID
		if _, exists := bindings[key]; exists || projects[project] {
			return nil, errBinding
		}
		item.TokenFile = filepath.Join(filepath.Dir(path), item.TokenFile)
		token, err := readToken(item.TokenFile)
		if err != nil {
			return nil, errBinding
		}
		item.credentialEpoch = sha256.Sum256([]byte(token))
		bindings[key] = item
		projects[project] = true
	}
	return bindings, nil
}

func protectedFile(path string, max int64) ([]byte, error) {
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm() != 0600 || info.Size() < 1 || info.Size() > max {
		return nil, errBinding
	}
	data, err := os.ReadFile(path)
	if err != nil || int64(len(data)) > max {
		return nil, errBinding
	}
	return data, nil
}

func readToken(path string) (string, error) {
	data, err := protectedFile(path, 8192)
	if err != nil {
		return "", errBinding
	}
	value := strings.TrimSpace(string(data))
	if len(strings.Split(value, ".")) != 3 || len(value) < 3 {
		return "", errBinding
	}
	for _, char := range value {
		if !(char >= 'a' && char <= 'z' || char >= 'A' && char <= 'Z' || char >= '0' && char <= '9' || char == '-' || char == '_' || char == '.') {
			return "", errBinding
		}
	}
	return value, nil
}
func safeID(value string, max int) bool {
	if len(value) < 1 || len(value) > max {
		return false
	}
	for _, char := range value {
		if !(char >= 'a' && char <= 'z' || char >= 'A' && char <= 'Z' || char >= '0' && char <= '9' || char == '_' || char == '-' || char == '.') {
			return false
		}
	}
	return true
}
