package config

import (
	"encoding/json"
	"errors"
	"github.com/triodelab/integration-corev2/internal/webvitals"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"strings"
)

const AlgoliaMCPResource = "https://mcp.algolia.com/mcp"

type AlgoliaClientRegistration struct {
	Version     int    `json:"version"`
	ClientID    string `json:"clientId"`
	RedirectURI string `json:"redirectUri"`
	Resource    string `json:"resource"`
}

var errAlgoliaRegistration = errors.New("Algolia requires a protected Integration-owned MCP client registration")
var algoliaClientID = regexp.MustCompile(`^[A-Za-z0-9_.-]{8,256}$`)

// AlgoliaRegistration never performs dynamic registration or accepts browser URLs.
func (c Config) AlgoliaRegistration() (AlgoliaClientRegistration, error) {
	p := c.AlgoliaMCPOAuthClientFile
	if !filepath.IsAbs(p) || filepath.Clean(p) != p {
		return AlgoliaClientRegistration{}, errAlgoliaRegistration
	}
	info, err := os.Lstat(p)
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm() != 0600 || info.Size() > 4096 || info.Size() < 1 {
		return AlgoliaClientRegistration{}, errAlgoliaRegistration
	}
	f, err := os.Open(p)
	if err != nil {
		return AlgoliaClientRegistration{}, errAlgoliaRegistration
	}
	defer f.Close()
	current, err := f.Stat()
	if err != nil || !os.SameFile(info, current) || current.Mode().Perm() != 0600 {
		return AlgoliaClientRegistration{}, errAlgoliaRegistration
	}
	raw, err := io.ReadAll(io.LimitReader(f, 4097))
	if err != nil || len(raw) > 4096 || webvitals.UniqueJSON(raw) != nil {
		return AlgoliaClientRegistration{}, errAlgoliaRegistration
	}
	var fields map[string]json.RawMessage
	if json.Unmarshal(raw, &fields) != nil || len(fields) != 4 {
		return AlgoliaClientRegistration{}, errAlgoliaRegistration
	}
	for _, key := range []string{"version", "clientId", "redirectUri", "resource"} {
		if _, ok := fields[key]; !ok {
			return AlgoliaClientRegistration{}, errAlgoliaRegistration
		}
	}
	var r AlgoliaClientRegistration
	expected := strings.TrimRight(c.PublicBaseURL, "/") + "/api/v1/providers/algolia/callback"
	if json.Unmarshal(raw, &r) != nil || r.Version != 1 || !algoliaClientID.MatchString(r.ClientID) || r.RedirectURI != expected || r.Resource != AlgoliaMCPResource {
		return AlgoliaClientRegistration{}, errAlgoliaRegistration
	}
	return r, nil
}
