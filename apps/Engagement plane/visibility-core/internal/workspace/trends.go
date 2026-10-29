package workspace

import (
	"bytes"
	"context"
	"encoding/json"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/trends"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"io"
	"net/http"
	"os"
	"strings"
	"time"
)

// Trends is public market research associated with a registered site, not that
// site's private search analytics. Integration owns the dev provider and cache.
func (s *Service) GetTrends(ctx context.Context, id, site string, revision int64, q trends.Query) (trends.Report, error) {
	if !trends.Valid(q) || revision < 1 || s.registry == nil {
		return trends.Report{}, trends.ErrInvalid
	}
	record, err := s.registry.Resolve(id, site)
	if err != nil {
		return trends.Report{}, err
	}
	if record.Website.Site != site || record.Website.ID != id || record.Website.Revision != revision {
		return trends.Report{}, websites.ErrConflict
	}
	if pulse.ValidateConfig(pulse.Config{IntegrationCoreURL: s.cfg.IntegrationCoreURL}) != nil || !boundToOrganization(s.cfg.IntegrationBearerTokenFile, s.registry.OrganizationID()) {
		return trends.Report{}, trends.ErrUnavailable
	}
	info, err := os.Lstat(s.cfg.IntegrationBearerTokenFile)
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm()&0077 != 0 || info.Size() < 1 || info.Size() > 8192 {
		return trends.Report{}, trends.ErrUnavailable
	}
	raw, err := os.ReadFile(s.cfg.IntegrationBearerTokenFile)
	token := strings.TrimSpace(string(raw))
	if err != nil || len(raw) > 8192 || token == "" || strings.ContainsAny(token, " \r\n\t") {
		return trends.Report{}, trends.ErrUnavailable
	}
	ctx, cancel := context.WithTimeout(ctx, 32*time.Second)
	defer cancel()
	body, _ := json.Marshal(q)
	req, err := http.NewRequestWithContext(ctx, "POST", s.cfg.IntegrationCoreURL+"/api/v1/insights/trends", bytes.NewReader(body))
	if err != nil {
		return trends.Report{}, trends.ErrUnavailable
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")
	client := &http.Client{Timeout: 32 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	res, err := client.Do(req)
	if err != nil {
		return trends.Report{}, trends.ErrUnavailable
	}
	defer res.Body.Close()
	payload, err := io.ReadAll(io.LimitReader(res.Body, 131073))
	if err != nil || len(payload) > 131072 || res.StatusCode != 200 {
		return trends.Report{}, trends.ErrUnavailable
	}
	var envelope struct {
		Success bool          `json:"success"`
		Data    trends.Report `json:"data"`
	}
	if json.Unmarshal(payload, &envelope) != nil || !envelope.Success || !trends.ValidReport(envelope.Data, q, time.Now()) {
		return trends.Report{}, trends.ErrUnavailable
	}
	latest, err := s.registry.Resolve(id, site)
	if err != nil || latest.Website.Revision != revision {
		return trends.Report{}, websites.ErrConflict
	}
	return envelope.Data, nil
}
