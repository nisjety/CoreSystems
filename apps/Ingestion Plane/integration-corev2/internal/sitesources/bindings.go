package sitesources

import (
	"crypto/sha256"
	"encoding/json"
	"errors"
	"io"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/triodelab/integration-corev2/internal/webvitals"
)

var errBinding = errors.New("site sources require protected valid deployment bindings and key files")
var keyPattern = regexp.MustCompile(`^[A-Za-z0-9_-]{16,256}$`)
var groupPattern = regexp.MustCompile(`^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$`)
var culturePattern = regexp.MustCompile(`^(default|[a-z]{2})$`)
var appPattern = regexp.MustCompile(`^[A-Z0-9]{8,32}$`)
var indexPattern = regexp.MustCompile(`^[A-Za-z0-9_.-]{1,128}$`)

type cookieBinding struct {
	DomainGroupID string `json:"domainGroupId"`
	Domain        string `json:"domain"`
	Culture       string `json:"culture"`
	APIKeyFile    string `json:"apiKeyFile"`
}
type algoliaBinding struct {
	ApplicationID    string `json:"applicationId"`
	IndexName        string `json:"indexName"`
	Region           string `json:"region"`
	AnalyticsKeyFile string `json:"analyticsKeyFile"`
}
type binding struct {
	OrganizationID            string          `json:"organizationId"`
	WebsiteID                 string          `json:"websiteId"`
	WebsiteRevision           int64           `json:"websiteRevision"`
	Site                      string          `json:"site"`
	Cookiebot                 *cookieBinding  `json:"cookiebot,omitempty"`
	Algolia                   *algoliaBinding `json:"algolia,omitempty"`
	cookieEpoch, algoliaEpoch [32]byte
}

func protected(path string, max int64) ([]byte, error) {
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
func readKey(path string) (string, error) {
	raw, err := protected(path, 512)
	if err != nil {
		return "", errBinding
	}
	key := strings.TrimSpace(string(raw))
	if !keyPattern.MatchString(key) {
		return "", errBinding
	}
	return key, nil
}
func bindingKey(org, id string) string { return org + "\n" + id }
func loadBindings(path string) (map[string]binding, error) {
	result := map[string]binding{}
	if path == "" {
		return result, nil
	}
	raw, err := protected(path, 64<<10)
	if err != nil || webvitals.UniqueJSON(raw) != nil || !exactKeys(raw, []string{"version", "installations"}, nil) {
		return nil, errBinding
	}
	var shape struct {
		Version       int               `json:"version"`
		Installations []json.RawMessage `json:"installations"`
	}
	if json.Unmarshal(raw, &shape) != nil || shape.Version != 1 || shape.Installations == nil || len(shape.Installations) > 32 {
		return nil, errBinding
	}
	for _, entry := range shape.Installations {
		if !exactKeys(entry, []string{"organizationId", "websiteId", "websiteRevision", "site"}, []string{"cookiebot", "algolia"}) {
			return nil, errBinding
		}
		var item binding
		if json.Unmarshal(entry, &item) != nil || !orgPattern.MatchString(item.OrganizationID) || !validInput(Input{WebsiteID: item.WebsiteID, Site: item.Site, WebsiteRevision: item.WebsiteRevision, Provider: "cookiebot"}) {
			return nil, errBinding
		}
		var fields map[string]json.RawMessage
		_ = json.Unmarshal(entry, &fields)
		if item.Cookiebot != nil {
			b := item.Cookiebot
			if !exactKeys(fields["cookiebot"], []string{"domainGroupId", "domain", "culture", "apiKeyFile"}, nil) || !groupPattern.MatchString(b.DomainGroupID) || !domainMatches(item.Site, b.Domain) || !culturePattern.MatchString(b.Culture) {
				return nil, errBinding
			}
			key, err := readKey(b.APIKeyFile)
			if err != nil {
				return nil, errBinding
			}
			item.cookieEpoch = sha256.Sum256([]byte(key))
		} else if _, present := fields["cookiebot"]; present {
			return nil, errBinding
		}
		if item.Algolia != nil {
			b := item.Algolia
			if !exactKeys(fields["algolia"], []string{"applicationId", "indexName", "region", "analyticsKeyFile"}, nil) || !appPattern.MatchString(b.ApplicationID) || !indexPattern.MatchString(b.IndexName) || (b.Region != "us" && b.Region != "de") {
				return nil, errBinding
			}
			key, err := readKey(b.AnalyticsKeyFile)
			if err != nil {
				return nil, errBinding
			}
			item.algoliaEpoch = sha256.Sum256([]byte(key))
		} else if _, present := fields["algolia"]; present {
			return nil, errBinding
		}
		key := bindingKey(item.OrganizationID, item.WebsiteID)
		if _, exists := result[key]; exists {
			return nil, errBinding
		}
		result[key] = item
	}
	return result, nil
}
func domainMatches(site, domain string) bool {
	parsed, err := url.Parse(site)
	if err != nil {
		return false
	}
	host := parsed.Hostname()
	return domain == host || domain == "www."+strings.TrimPrefix(host, "www.") || "www."+domain == host
}
func sameBinding(a, b binding) bool {
	if a.OrganizationID != b.OrganizationID || a.WebsiteID != b.WebsiteID || a.WebsiteRevision != b.WebsiteRevision || a.Site != b.Site || a.cookieEpoch != b.cookieEpoch || a.algoliaEpoch != b.algoliaEpoch {
		return false
	}
	if (a.Cookiebot == nil) != (b.Cookiebot == nil) || (a.Algolia == nil) != (b.Algolia == nil) {
		return false
	}
	if a.Cookiebot != nil && *a.Cookiebot != *b.Cookiebot {
		return false
	}
	return a.Algolia == nil || *a.Algolia == *b.Algolia
}
