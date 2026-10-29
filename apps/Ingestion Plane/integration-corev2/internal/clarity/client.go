package clarity

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"math"
	"math/big"
	"net/http"
	"strconv"
	"strings"

	"github.com/triodelab/integration-corev2/internal/egress"
)

const endpoint = "https://www.clarity.ms/export-data/api/v1/project-live-insights"
const maxResponseBytes = 1 << 20

var errNoData = errors.New("Clarity has no data for this rolling window")

type Metrics struct {
	Sessions        *int64     `json:"sessions"`
	BotSessions     *int64     `json:"botSessions"`
	Users           *int64     `json:"users"`
	PagesPerSession *float64   `json:"pagesPerSession"`
	UX              *UXMetrics `json:"ux,omitempty"`
}

// Project-level session percentages and averages, never individual replays.
type UXMetrics struct {
	DeadClickPercent       *float64 `json:"deadClickPercent"`
	RageClickPercent       *float64 `json:"rageClickPercent"`
	ExcessiveScrollPercent *float64 `json:"excessiveScrollPercent"`
	QuickBackPercent       *float64 `json:"quickBackPercent"`
	ScriptErrorPercent     *float64 `json:"scriptErrorPercent"`
	ErrorClickPercent      *float64 `json:"errorClickPercent"`
	AverageScrollDepth     *float64 `json:"averageScrollDepth"`
	ActiveSeconds          *float64 `json:"activeSeconds"`
	TotalSeconds           *float64 `json:"totalSeconds"`
}

func exportClient(client *http.Client) *http.Client {
	if client == nil {
		client = egress.SafeClient(egress.ClientConfig{RequestTimeout: 8 * second})
	}
	copy := *client
	copy.Timeout = 8 * second
	copy.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	return &copy
}

func fetch(ctx context.Context, client *http.Client, item binding, days int) (*Metrics, string) {
	token, err := readToken(item.TokenFile)
	if err != nil {
		return nil, "authorization_required"
	}
	req, err := http.NewRequestWithContext(ctx, "GET", endpoint+"?numOfDays="+strconv.Itoa(days), nil)
	if err != nil {
		return nil, "source_unavailable"
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Accept", "application/json")
	response, err := client.Do(req)
	if err != nil {
		return nil, "source_unavailable"
	}
	defer response.Body.Close()
	switch response.StatusCode {
	case 401, 403:
		return nil, "authorization_required"
	case 429:
		return nil, "quota_limited"
	case 200:
	default:
		return nil, "source_unavailable"
	}
	data, err := io.ReadAll(io.LimitReader(response.Body, maxResponseBytes+1))
	if err != nil || len(data) > maxResponseBytes {
		return nil, "source_unavailable"
	}
	metrics, err := parseTraffic(data)
	if err != nil {
		if errors.Is(err, errNoData) {
			return nil, "no_data"
		}
		return nil, "source_unavailable"
	}
	return metrics, ""
}

func parseTraffic(data []byte) (*Metrics, error) {
	var reports []struct {
		Name        string                       `json:"metricName"`
		Information []map[string]json.RawMessage `json:"information"`
	}
	if json.Unmarshal(data, &reports) != nil || len(reports) > 64 {
		return nil, errBinding
	}
	if reports == nil {
		return nil, errBinding
	}
	if len(reports) == 0 {
		return nil, errNoData
	}
	var result *Metrics
	for _, report := range reports {
		if report.Name != "Traffic" {
			continue
		}
		if report.Information != nil && len(report.Information) == 0 {
			return nil, errNoData
		}
		if result != nil || len(report.Information) != 1 {
			return nil, errBinding
		}
		info := report.Information[0]
		sessions, err := integer(info, "totalSessionCount")
		if err != nil {
			return nil, err
		}
		bots, err := integer(info, "totalBotSessionCount")
		if err != nil {
			return nil, err
		}
		users, err := integer(info, "distinctUserCount", "distantUserCount")
		if err != nil {
			return nil, err
		}
		pages, err := number(info, "pagesPerSessionPercentage", "PagesPerSessionPercentage")
		if err != nil {
			return nil, err
		}
		if sessions == nil && bots == nil && users == nil && pages == nil {
			return nil, errBinding
		}
		result = &Metrics{Sessions: sessions, BotSessions: bots, Users: users, PagesPerSession: pages}
	}
	if result == nil {
		return nil, errBinding
	}
	ux := UXMetrics{}
	seen := map[string]bool{}
	for _, report := range reports {
		var fields map[string]**float64
		switch report.Name {
		case "DeadClickCount":
			fields = map[string]**float64{"sessionsWithMetricPercentage": &ux.DeadClickPercent}
		case "RageClickCount":
			fields = map[string]**float64{"sessionsWithMetricPercentage": &ux.RageClickPercent}
		case "ExcessiveScroll":
			fields = map[string]**float64{"sessionsWithMetricPercentage": &ux.ExcessiveScrollPercent}
		case "QuickbackClick":
			fields = map[string]**float64{"sessionsWithMetricPercentage": &ux.QuickBackPercent}
		case "ScriptErrorCount":
			fields = map[string]**float64{"sessionsWithMetricPercentage": &ux.ScriptErrorPercent}
		case "ErrorClickCount":
			fields = map[string]**float64{"sessionsWithMetricPercentage": &ux.ErrorClickPercent}
		case "ScrollDepth":
			fields = map[string]**float64{"averageScrollDepth": &ux.AverageScrollDepth}
		case "EngagementTime":
			fields = map[string]**float64{"activeTime": &ux.ActiveSeconds, "totalTime": &ux.TotalSeconds}
		default:
			continue
		}
		if seen[report.Name] || len(report.Information) > 1 {
			return nil, errBinding
		}
		seen[report.Name] = true
		if len(report.Information) == 0 {
			continue
		}
		for key, target := range fields {
			value, err := number(report.Information[0], key)
			if err != nil || value != nil && report.Name != "EngagementTime" && *value > 100 {
				return nil, errBinding
			}
			*target = value
		}
	}
	if ux.ActiveSeconds != nil && ux.TotalSeconds != nil && *ux.ActiveSeconds > *ux.TotalSeconds {
		return nil, errBinding
	}
	if len(seen) > 0 {
		result.UX = &ux
	}
	return result, nil
}

func integer(info map[string]json.RawMessage, keys ...string) (*int64, error) {
	value, err := number(info, keys...)
	if err != nil || value == nil {
		return nil, err
	}
	if *value != math.Trunc(*value) {
		return nil, errBinding
	}
	// Parse the decimal representation exactly too: float64 can round a
	// fractional count close to 2^53 into an apparently valid integer.
	for _, key := range keys {
		raw, exists := info[key]
		if !exists || bytes.Equal(raw, []byte("null")) {
			continue
		}
		var text string
		if json.Unmarshal(raw, &text) != nil {
			text = string(raw)
		}
		if index := strings.IndexAny(text, "eE"); index >= 0 {
			exponent, err := strconv.Atoi(text[index+1:])
			if err != nil || exponent < -64 || exponent > 64 {
				return nil, errBinding
			}
		}
		exact, ok := new(big.Rat).SetString(text)
		if !ok || !exact.IsInt() || exact.Sign() < 0 || exact.Num().BitLen() > 53 {
			return nil, errBinding
		}
	}
	parsed := int64(*value)
	return &parsed, nil
}

func number(info map[string]json.RawMessage, keys ...string) (*float64, error) {
	var result *float64
	for _, key := range keys {
		raw, exists := info[key]
		if !exists || bytes.Equal(raw, []byte("null")) {
			continue
		}
		var text string
		if json.Unmarshal(raw, &text) != nil {
			text = string(raw)
		}
		if len(text) > 64 {
			return nil, errBinding
		}
		value, err := strconv.ParseFloat(text, 64)
		if err != nil || math.IsNaN(value) || math.IsInf(value, 0) || value < 0 || value > 9007199254740991 {
			return nil, errBinding
		}
		if result != nil && *result != value {
			return nil, errBinding
		}
		result = &value
	}
	return result, nil
}
