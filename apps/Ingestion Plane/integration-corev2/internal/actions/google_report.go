package actions

import (
	"context"
	"errors"
	"net/url"
	"strings"
	"time"

	"github.com/triodelab/integration-corev2/internal/pagespeed"
)

var errAnalyticsReport = errors.New("Google Analytics report unavailable")

// executeGoogleAnalyticsReport performs three bounded fixed reads. The stream
// is verified against the submitted origin before reading event data. Summary
// and daily reports are separate so daily unique users are never summed.
func (s *Service) executeGoogleAnalyticsReport(ctx context.Context, token string, params, body map[string]any) (any, error) {
	property, _ := params["propertyId"].(string)
	stream, _ := params["streamId"].(string)
	site, _ := params["site"].(string)
	reportingSite := site
	value, hasReportingSite := params["reportingSite"]
	if hasReportingSite {
		reportingSite, _ = value.(string)
	}
	if len(params) != 3 && len(params) != 4 || len(params) == 3 && hasReportingSite || len(params) == 4 && !hasReportingSite || !analyticsNumericID(property) || !analyticsNumericID(stream) || pagespeed.ValidateInput(site, "mobile") != nil || !allowedGA4ReportingSite(site, reportingSite) {
		return nil, errAnalyticsReport
	}
	dates, err := analyticsReportDates(body, time.Now().UTC())
	if err != nil {
		return nil, err
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
	origin, _ := web["defaultUri"].(string)
	if !allowedGA4ReportingSite(site, strings.TrimSuffix(origin, "/")+"/") {
		return nil, errAnalyticsReport
	}
	data := strings.TrimRight(s.cfg.GoogleAnalyticsDataAPIBaseURL, "/")
	if data == "" {
		data = "https://analyticsdata.googleapis.com"
	}
	endpoint := data + "/v1beta/properties/" + property + ":runReport"
	base := analyticsReportPayload(reportingSite, stream, dates)
	summary, err := s.postBearer(ctx, token, endpoint, base, nil)
	if err != nil {
		return nil, errAnalyticsReport
	}
	dailyPayload := analyticsReportPayload(reportingSite, stream, dates)
	dailyPayload["dimensions"] = []any{map[string]string{"name": "date"}}
	dailyPayload["orderBys"] = []any{map[string]any{"dimension": map[string]string{"dimensionName": "date"}, "desc": false}}
	dailyPayload["limit"] = "366"
	daily, err := s.postBearer(ctx, token, endpoint, dailyPayload, nil)
	if err != nil {
		return nil, errAnalyticsReport
	}
	return map[string]any{"summary": summary, "daily": daily}, nil
}

func allowedGA4ReportingSite(site, reportingSite string) bool {
	if pagespeed.ValidateInput(reportingSite, "mobile") != nil {
		return false
	}
	if reportingSite == site {
		return true
	}
	u, err := url.Parse(site)
	if err != nil || pagespeed.ValidateInput(site, "mobile") != nil {
		return false
	}
	host := u.Hostname()
	if strings.HasPrefix(host, "www.") {
		return reportingSite == "https://"+strings.TrimPrefix(host, "www.")+"/"
	}
	return reportingSite == "https://www."+host+"/"
}

func analyticsReportDates(body map[string]any, now time.Time) (map[string]string, error) {
	if len(body) != 2 {
		return nil, errAnalyticsReport
	}
	start, _ := body["startDate"].(string)
	end, _ := body["endDate"].(string)
	first, err := time.Parse("2006-01-02", start)
	if err != nil {
		return nil, errAnalyticsReport
	}
	last, err := time.Parse("2006-01-02", end)
	today := time.Date(now.UTC().Year(), now.UTC().Month(), now.UTC().Day(), 0, 0, 0, 0, time.UTC)
	if err != nil || first.After(last) || last.Sub(first) > 365*24*time.Hour || !last.Before(today) {
		return nil, errAnalyticsReport
	}
	return map[string]string{"startDate": start, "endDate": end}, nil
}

func analyticsReportPayload(site, stream string, dates map[string]string) map[string]any {
	metrics := []any{}
	for _, name := range []string{"totalUsers", "sessions", "engagedSessions", "screenPageViews", "keyEvents", "userEngagementDuration"} {
		metrics = append(metrics, map[string]string{"name": name})
	}
	filter := func(field, value string) map[string]any {
		return map[string]any{"filter": map[string]any{"fieldName": field, "stringFilter": map[string]any{"matchType": "EXACT", "value": value, "caseSensitive": false}}}
	}
	host := strings.TrimSuffix(strings.TrimPrefix(site, "https://"), "/")
	return map[string]any{"dateRanges": []any{dates}, "metrics": metrics, "dimensionFilter": map[string]any{"andGroup": map[string]any{"expressions": []any{filter("hostName", host), filter("streamId", stream)}}}, "limit": "1", "keepEmptyRows": false, "returnPropertyQuota": false}
}
