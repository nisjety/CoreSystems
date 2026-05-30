package repoprovider

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"time"
)

// SafeBrowsingProvider implements Google Safe Browsing API v4
type SafeBrowsingProvider struct {
	APIKey     string
	BaseURL    string
	HTTPClient *http.Client
	enabled    bool
}

// SafeBrowsingRequest represents the API request structure
type SafeBrowsingRequest struct {
	Client struct {
		ClientID      string `json:"clientId"`
		ClientVersion string `json:"clientVersion"`
	} `json:"client"`
	ThreatInfo struct {
		ThreatTypes      []string            `json:"threatTypes"`
		PlatformTypes    []string            `json:"platformTypes"`
		ThreatEntryTypes []string            `json:"threatEntryTypes"`
		ThreatEntries    []map[string]string `json:"threatEntries"`
	} `json:"threatInfo"`
}

// SafeBrowsingResponse represents the API response structure
type SafeBrowsingResponse struct {
	Matches []struct {
		ThreatType      string `json:"threatType"`
		PlatformType    string `json:"platformType"`
		ThreatEntryType string `json:"threatEntryType"`
		Threat          struct {
			URL string `json:"url"`
		} `json:"threat"`
		ThreatEntryMetadata struct {
			Entries []struct {
				Key   string `json:"key"`
				Value string `json:"value"`
			} `json:"entries"`
		} `json:"threatEntryMetadata"`
		CacheDuration string `json:"cacheDuration"`
	} `json:"matches"`
}

// NewSafeBrowsingProvider creates a new Safe Browsing provider
func NewSafeBrowsingProvider(apiKey string) *SafeBrowsingProvider {
	return &SafeBrowsingProvider{
		APIKey:  apiKey,
		BaseURL: "https://safebrowsing.googleapis.com/v4/threatMatches:find",
		HTTPClient: &http.Client{
			Timeout: 10 * time.Second,
		},
		enabled: apiKey != "",
	}
}

// CheckURL checks a URL against Google Safe Browsing
func (sbp *SafeBrowsingProvider) CheckURL(ctx context.Context, url string) (*ReputationResult, error) {
	startTime := time.Now()

	result := &ReputationResult{
		Provider:   "Google Safe Browsing",
		URL:        url,
		CheckedAt:  startTime,
		Level:      Clean,
		Confidence: 0.9,
	}

	if !sbp.enabled {
		result.Error = "API key not configured"
		result.Level = Unknown
		return result, fmt.Errorf("Safe Browsing API key not configured")
	}

	// Prepare request
	requestBody := SafeBrowsingRequest{}
	requestBody.Client.ClientID = "secure-url-checker"
	requestBody.Client.ClientVersion = "1.0.0"

	requestBody.ThreatInfo.ThreatTypes = []string{
		"MALWARE",
		"SOCIAL_ENGINEERING",
		"UNWANTED_SOFTWARE",
		"POTENTIALLY_HARMFUL_APPLICATION",
	}
	requestBody.ThreatInfo.PlatformTypes = []string{"ANY_PLATFORM"}
	requestBody.ThreatInfo.ThreatEntryTypes = []string{"URL"}
	requestBody.ThreatInfo.ThreatEntries = []map[string]string{
		{"url": url},
	}

	// Marshal request
	jsonData, err := json.Marshal(requestBody)
	if err != nil {
		result.Error = err.Error()
		result.Level = Unknown
		return result, err
	}

	// Create HTTP request
	apiURL := fmt.Sprintf("%s?key=%s", sbp.BaseURL, sbp.APIKey)
	req, err := http.NewRequestWithContext(ctx, "POST", apiURL, bytes.NewBuffer(jsonData))
	if err != nil {
		result.Error = err.Error()
		result.Level = Unknown
		return result, err
	}

	req.Header.Set("Content-Type", "application/json")

	// Make request
	resp, err := sbp.HTTPClient.Do(req)
	if err != nil {
		result.Error = err.Error()
		result.Level = Unknown
		return result, err
	}
	defer resp.Body.Close()

	result.ResponseTime = time.Since(startTime)

	// Handle HTTP errors
	if resp.StatusCode != http.StatusOK {
		result.Error = fmt.Sprintf("HTTP %d", resp.StatusCode)
		result.Level = Unknown
		return result, fmt.Errorf("API returned HTTP %d", resp.StatusCode)
	}

	// Parse response
	var response SafeBrowsingResponse
	if err := json.NewDecoder(resp.Body).Decode(&response); err != nil {
		result.Error = err.Error()
		result.Level = Unknown
		return result, err
	}

	// Process results
	if len(response.Matches) == 0 {
		result.Level = Clean
		result.Description = "No threats detected"
		result.Score = 0.0
	} else {
		result.Level = Blacklisted
		result.Score = 1.0

		var categories []string
		for _, match := range response.Matches {
			categories = append(categories, match.ThreatType)
		}
		result.Categories = categories
		result.Description = fmt.Sprintf("Detected %d threat(s)", len(response.Matches))
	}

	return result, nil
}

// CheckDomain checks a domain against Google Safe Browsing
func (sbp *SafeBrowsingProvider) CheckDomain(ctx context.Context, domain string) (*ReputationResult, error) {
	return sbp.CheckURL(ctx, "http://"+domain)
}

// Name returns the provider name
func (sbp *SafeBrowsingProvider) Name() string {
	return "Google Safe Browsing"
}

// IsAvailable checks if the provider is available
func (sbp *SafeBrowsingProvider) IsAvailable() bool {
	return sbp.enabled
}

// GetRateLimit returns the rate limit information
func (sbp *SafeBrowsingProvider) GetRateLimit() RateLimit {
	return RateLimit{
		RequestsPerMinute: 600, // 10,000 requests per day
		RequestsPerHour:   600,
		RequestsPerDay:    10000,
		BurstAllowed:      10,
	}
}
