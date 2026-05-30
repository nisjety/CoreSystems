package repoprovider

import (
	"encoding/json"
	"fmt"
	"net"
	"net/http"
	"strconv"
	"time"
)

// AbuseIPDBProvider implements IPProvider for AbuseIPDB API
type AbuseIPDBProvider struct {
	apiKey     string
	httpClient *http.Client
}

// AbuseIPDBResponse represents the response from AbuseIPDB API
type AbuseIPDBResponse struct {
	Data struct {
		IPAddress            string `json:"ipAddress"`
		IsPublic             bool   `json:"isPublic"`
		IPVersion            int    `json:"ipVersion"`
		IsWhitelisted        bool   `json:"isWhitelisted"`
		AbuseConfidenceLevel int    `json:"abuseConfidenceLevel"`
		CountryCode          string `json:"countryCode"`
		CountryName          string `json:"countryName"`
		UsageType            string `json:"usageType"`
		ISP                  string `json:"isp"`
		Domain               string `json:"domain"`
		TotalReports         int    `json:"totalReports"`
		NumDistinctUsers     int    `json:"numDistinctUsers"`
		LastReportedAt       string `json:"lastReportedAt"`
	} `json:"data"`
}

// NewAbuseIPDBProvider creates a new AbuseIPDB provider
func NewAbuseIPDBProvider(apiKey string) *AbuseIPDBProvider {
	return &AbuseIPDBProvider{
		apiKey: apiKey,
		httpClient: &http.Client{
			Timeout: 10 * time.Second,
		},
	}
}

// CheckIP checks the reputation of an IP address
func (a *AbuseIPDBProvider) CheckIP(ip string) (*ReputationResult, error) {
	// Validate IP address
	if net.ParseIP(ip) == nil {
		return nil, fmt.Errorf("invalid IP address: %s", ip)
	}

	// Build API request
	apiURL := fmt.Sprintf("https://api.abuseipdb.com/api/v2/check?ipAddress=%s&maxAgeInDays=90&verbose", ip)

	startTime := time.Now()
	req, err := http.NewRequest("GET", apiURL, nil)
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}

	req.Header.Set("Key", a.apiKey)
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", "URL Security Scanner v1.0")

	resp, err := a.httpClient.Do(req)
	responseTime := time.Since(startTime)
	if err != nil {
		return nil, fmt.Errorf("failed to make request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("API returned status %d", resp.StatusCode)
	}

	var abuseResp AbuseIPDBResponse
	if err := json.NewDecoder(resp.Body).Decode(&abuseResp); err != nil {
		return nil, fmt.Errorf("failed to decode response: %w", err)
	}

	// Process results
	result := &ReputationResult{
		Provider:     "AbuseIPDB",
		IP:           ip,
		CheckedAt:    time.Now(),
		ResponseTime: responseTime,
		Metadata: map[string]string{
			"country_code":   abuseResp.Data.CountryCode,
			"country_name":   abuseResp.Data.CountryName,
			"usage_type":     abuseResp.Data.UsageType,
			"isp":            abuseResp.Data.ISP,
			"domain":         abuseResp.Data.Domain,
			"total_reports":  strconv.Itoa(abuseResp.Data.TotalReports),
			"distinct_users": strconv.Itoa(abuseResp.Data.NumDistinctUsers),
		},
	}

	// Parse last reported time
	if abuseResp.Data.LastReportedAt != "" {
		if lastReported, err := time.Parse("2006-01-02T15:04:05-07:00", abuseResp.Data.LastReportedAt); err == nil {
			result.LastSeen = &lastReported
		}
	}

	// Determine threat level based on abuse confidence
	confidenceLevel := abuseResp.Data.AbuseConfidenceLevel
	totalReports := abuseResp.Data.TotalReports

	if abuseResp.Data.IsWhitelisted {
		result.Level = Clean
		result.Score = 0.0
		result.Confidence = 0.9
		result.Description = "IP is whitelisted"
	} else if confidenceLevel >= 75 && totalReports >= 10 {
		result.Level = Blacklisted
		result.Score = float64(confidenceLevel) / 100.0
		result.Confidence = 0.9
		result.Description = fmt.Sprintf("High abuse confidence: %d%% (%d reports)", confidenceLevel, totalReports)
		result.Categories = a.determineCategories(confidenceLevel, totalReports)
	} else if confidenceLevel >= 25 || totalReports >= 3 {
		result.Level = Suspicious
		result.Score = float64(confidenceLevel) / 100.0
		result.Confidence = 0.7
		result.Description = fmt.Sprintf("Moderate abuse confidence: %d%% (%d reports)", confidenceLevel, totalReports)
		result.Categories = a.determineCategories(confidenceLevel, totalReports)
	} else {
		result.Level = Clean
		result.Score = float64(confidenceLevel) / 100.0
		result.Confidence = 0.8
		if totalReports > 0 {
			result.Description = fmt.Sprintf("Low abuse confidence: %d%% (%d reports)", confidenceLevel, totalReports)
		} else {
			result.Description = "No abuse reports found"
		}
	}

	return result, nil
}

// determineCategories returns threat categories based on abuse data
func (a *AbuseIPDBProvider) determineCategories(confidence, reports int) []string {
	categories := []string{}

	if confidence >= 50 {
		categories = append(categories, "malicious")
	}
	if reports >= 10 {
		categories = append(categories, "spam")
	}
	if confidence >= 75 {
		categories = append(categories, "botnet")
	}

	return categories
}

// GetProviderName returns the provider name
func (a *AbuseIPDBProvider) GetProviderName() string {
	return "AbuseIPDB"
}
