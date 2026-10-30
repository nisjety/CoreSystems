package actions

import (
	"context"
	"fmt"
	"math"
	"net/url"
	"strconv"
	"strings"
)

// These two fixed Admin API reads discover existing resources only. They do
// not accept a provider URL, create resources, or infer website ownership.
func (s *Service) executeGoogleAnalyticsRead(ctx context.Context, token, operation string, params, body map[string]any) (any, error) {
	if len(body) != 0 {
		return nil, fmt.Errorf("GA4 discovery body must be empty")
	}
	values, err := analyticsPagination(params)
	if err != nil {
		return nil, err
	}
	path := "/v1beta/accountSummaries"
	if operation == "ga4.data_streams.list" {
		id, _ := params["propertyId"].(string)
		if !analyticsNumericID(id) {
			return nil, fmt.Errorf("propertyId must be a numeric GA4 property ID")
		}
		path = "/v1beta/properties/" + id + "/dataStreams"
	}
	for key := range params {
		if key != "pageSize" && key != "pageToken" && !(key == "propertyId" && operation == "ga4.data_streams.list") {
			return nil, fmt.Errorf("unsupported GA4 discovery parameter")
		}
	}
	base := strings.TrimRight(s.cfg.GoogleAnalyticsAdminAPIBaseURL, "/")
	if base == "" {
		base = "https://analyticsadmin.googleapis.com"
	}
	return s.getBearer(ctx, token, base+path+"?"+values.Encode(), nil)
}

func analyticsPagination(params map[string]any) (url.Values, error) {
	size := 100
	if raw, exists := params["pageSize"]; exists {
		switch value := raw.(type) {
		case int:
			size = value
		case float64:
			if math.IsNaN(value) || math.IsInf(value, 0) || value != math.Trunc(value) || value < 1 || value > 200 {
				return nil, fmt.Errorf("pageSize must be an integer between 1 and 200")
			}
			size = int(value)
		default:
			return nil, fmt.Errorf("pageSize must be an integer")
		}
	}
	if size < 1 || size > 200 {
		return nil, fmt.Errorf("pageSize must be between 1 and 200")
	}
	values := url.Values{"pageSize": {strconv.Itoa(size)}}
	if raw, exists := params["pageToken"]; exists {
		value, ok := raw.(string)
		if !ok || len(value) > 2048 {
			return nil, fmt.Errorf("invalid GA4 page token")
		}
		for _, char := range value {
			if char < 33 || char > 126 {
				return nil, fmt.Errorf("invalid GA4 page token")
			}
		}
		if value != "" {
			values.Set("pageToken", value)
		}
	}
	return values, nil
}

func analyticsNumericID(value string) bool {
	if len(value) == 0 || len(value) > 30 {
		return false
	}
	for _, char := range value {
		if char < '0' || char > '9' {
			return false
		}
	}
	return true
}
