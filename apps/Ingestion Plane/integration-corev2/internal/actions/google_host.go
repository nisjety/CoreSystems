package actions

import (
	"context"
	"encoding/json"
	"strconv"
	"strings"
	"time"

	"github.com/triodelab/integration-corev2/internal/pagespeed"
)

// Resolve within an accessible GA4 web stream, never by forwarding credentials
// to a website. Only the exact selected/www pair is queried; counts stay private.
func (s *Service) executeGoogleAnalyticsHost(ctx context.Context, token string, params, body map[string]any) (any, error) {
	site, _ := params["site"].(string)
	property, _ := params["propertyId"].(string)
	stream, _ := params["streamId"].(string)
	if len(params) != 3 || len(body) != 0 || pagespeed.ValidateInput(site, "mobile") != nil || !analyticsNumericID(property) || !analyticsNumericID(stream) {
		return nil, errAnalyticsReport
	}
	ctx, cancel := context.WithTimeout(ctx, 8*time.Second)
	defer cancel()
	admin := strings.TrimRight(s.cfg.GoogleAnalyticsAdminAPIBaseURL, "/")
	if admin == "" {
		admin = "https://analyticsadmin.googleapis.com"
	}
	resource := "properties/" + property + "/dataStreams/" + stream
	verified, err := s.getBearerMap(ctx, token, admin+"/v1beta/"+resource, nil)
	if err != nil || verified["name"] != resource || verified["type"] != "WEB_DATA_STREAM" {
		return nil, errAnalyticsReport
	}
	web, _ := verified["webStreamData"].(map[string]any)
	uri, _ := web["defaultUri"].(string)
	configured := strings.TrimSuffix(uri, "/") + "/"
	if !allowedGA4ReportingSite(site, configured) {
		return nil, errAnalyticsReport
	}
	host := strings.TrimSuffix(strings.TrimPrefix(site, "https://"), "/")
	other := "www." + host
	if strings.HasPrefix(host, "www.") {
		other = strings.TrimPrefix(host, "www.")
	}
	today := time.Now().UTC()
	end := today.AddDate(0, 0, -1)
	start := end.AddDate(0, 0, -27)
	payload := analyticsReportPayload(site, stream, map[string]string{"startDate": start.Format("2006-01-02"), "endDate": end.Format("2006-01-02")})
	payload["metrics"] = []any{map[string]string{"name": "sessions"}}
	payload["dimensions"] = []any{map[string]string{"name": "hostName"}}
	payload["limit"] = "2"
	filter := payload["dimensionFilter"].(map[string]any)["andGroup"].(map[string]any)["expressions"].([]any)
	filter[0] = map[string]any{"filter": map[string]any{"fieldName": "hostName", "inListFilter": map[string]any{"values": []string{host, other}, "caseSensitive": false}}}
	base := strings.TrimRight(s.cfg.GoogleAnalyticsDataAPIBaseURL, "/")
	if base == "" {
		base = "https://analyticsdata.googleapis.com"
	}
	result, err := s.postBearer(ctx, token, base+"/v1beta/properties/"+property+":runReport", payload, nil)
	if err != nil {
		return nil, errAnalyticsReport
	}
	raw, err := json.Marshal(result)
	if err != nil {
		return nil, errAnalyticsReport
	}
	var report struct {
		Dimensions []struct {
			Name string `json:"name"`
		} `json:"dimensionHeaders"`
		Metrics []struct {
			Name string `json:"name"`
		} `json:"metricHeaders"`
		Rows []struct {
			Dimensions []struct {
				Value string `json:"value"`
			} `json:"dimensionValues"`
			Metrics []struct {
				Value string `json:"value"`
			} `json:"metricValues"`
		} `json:"rows"`
		RowCount int `json:"rowCount"`
	}
	if json.Unmarshal(raw, &report) != nil || len(report.Dimensions) != 1 || report.Dimensions[0].Name != "hostName" || len(report.Metrics) != 1 || report.Metrics[0].Name != "sessions" || len(report.Rows) > 2 || report.RowCount > 2 || report.RowCount < 0 {
		return nil, errAnalyticsReport
	}
	active := map[string]bool{}
	seen := map[string]bool{}
	for _, row := range report.Rows {
		if len(row.Dimensions) != 1 || len(row.Metrics) != 1 {
			return nil, errAnalyticsReport
		}
		name := strings.ToLower(row.Dimensions[0].Value)
		count, err := strconv.ParseUint(row.Metrics[0].Value, 10, 53)
		if err != nil || (name != host && name != other) || seen[name] {
			return nil, errAnalyticsReport
		}
		seen[name] = true
		if count > 0 {
			active[name] = true
		}
	}
	selected := configured
	if active[host] {
		selected = site
	} else if active[other] {
		selected = "https://" + other + "/"
	}
	return map[string]any{"reportingSite": selected}, nil
}
