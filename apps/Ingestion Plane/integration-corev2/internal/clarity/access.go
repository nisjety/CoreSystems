package clarity

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"strings"

	"github.com/triodelab/integration-corev2/internal/pagespeed"
)

// BindingAccess describes only current operator-confirmed access. It is not
// domain ownership proof and never contacts Microsoft or exposes credentials.
type BindingAccess struct {
	Site       string `json:"site"`
	Configured bool   `json:"configured"`
	ProjectID  string `json:"projectId,omitempty"`
	Version    string `json:"version,omitempty"`
}

func (s *Service) Access(org, site string) (BindingAccess, error) {
	access := BindingAccess{Site: site}
	item, exists, err := s.currentBinding(org, site)
	if err != nil || !exists {
		return access, err
	}
	return BindingAccess{Site: site, Configured: true, ProjectID: item.ProjectID, Version: bindingVersion(item)}, nil
}

func (s *Service) currentBinding(org, site string) (binding, bool, error) {
	if !safeID(org, 128) || pagespeed.ValidateInput(site, "mobile") != nil {
		return binding{}, false, errBinding
	}
	// Reload on every authority check: revocation and credential removal must not
	// wait for a restart or the six-hour aggregate-report cache to expire.
	bindings, err := loadBindings(s.bindingsPath)
	if err != nil {
		return binding{}, false, errBinding
	}
	item, exists := bindings[org+"\n"+site]
	if !exists {
		host := strings.TrimSuffix(strings.TrimPrefix(site, "https://"), "/")
		if strings.HasPrefix(host, "www.") {
			host = strings.TrimPrefix(host, "www.")
		} else {
			host = "www." + host
		}
		alternate := "https://" + host + "/"
		if pagespeed.ValidateInput(alternate, "mobile") == nil {
			item, exists = bindings[org+"\n"+alternate]
		}
	}
	return item, exists, nil
}

func bindingVersion(item binding) string {
	// Only approved non-secret binding fields enter the public version. Token
	// rotation uses a separate process-private cache epoch, never serialized.
	data, _ := json.Marshal([]string{item.OrganizationID, item.Site, item.ProjectID, item.ConfirmedAt})
	digest := sha256.Sum256(data)
	return hex.EncodeToString(digest[:])
}
