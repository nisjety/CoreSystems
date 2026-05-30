package repoprovider

import (
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"time"
)

// PhishTankProvider implements URLProvider for PhishTank API
type PhishTankProvider struct {
	apiKey     string
	httpClient *http.Client
}

// PhishTankResponse represents the response from PhishTank API
type PhishTankResponse struct {
	Meta struct {
		QueryTime string `json:"query_time"`
		Status    string `json:"status"`
	} `json:"meta"`
	Results struct {
		URL        string `json:"url"`
		InDatabase bool   `json:"in_database"`
		PhishID    string `json:"phish_id,omitempty"`
		Detail     string `json:"detail,omitempty"`
		Verified   bool   `json:"verified,omitempty"`
		Valid      bool   `json:"valid,omitempty"`
	} `json:"results"`
}

// NewPhishTankProvider creates a new PhishTank provider
func NewPhishTankProvider(apiKey string) *PhishTankProvider {
	return &PhishTankProvider{
		apiKey: apiKey,
		httpClient: &http.Client{
			Timeout: 10 * time.Second,
		},
	}
}

// CheckURL checks if a URL is in PhishTank's database
func (p *PhishTankProvider) CheckURL(checkURL string) (*ReputationResult, error) {
	// Clean and encode the URL
	checkURL = strings.TrimSpace(checkURL)
	if !strings.HasPrefix(checkURL, "http://") && !strings.HasPrefix(checkURL, "https://") {
		checkURL = "http://" + checkURL
	}

	encodedURL := url.QueryEscape(checkURL)

	// Build API request URL
	var apiURL string
	if p.apiKey != "" {
		apiURL = fmt.Sprintf("https://checkurl.phishtank.com/checkurl/?url=%s&format=json&app_key=%s", encodedURL, p.apiKey)
	} else {
		// Use public API (limited rate)
		apiURL = fmt.Sprintf("https://checkurl.phishtank.com/checkurl/?url=%s&format=json", encodedURL)
	}

	// Make request
	startTime := time.Now()
	req, err := http.NewRequest("POST", apiURL, nil)
	if err != nil {
		return nil, fmt.Errorf("failed to create request: %w", err)
	}

	req.Header.Set("User-Agent", "URL Security Scanner v1.0")

	resp, err := p.httpClient.Do(req)
	responseTime := time.Since(startTime)
	if err != nil {
		return nil, fmt.Errorf("failed to make request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("API returned status %d", resp.StatusCode)
	}

	var phishTankResp PhishTankResponse
	if err := json.NewDecoder(resp.Body).Decode(&phishTankResp); err != nil {
		return nil, fmt.Errorf("failed to decode response: %w", err)
	}

	// Process results
	result := &ReputationResult{
		Provider:     "PhishTank",
		URL:          checkURL,
		Level:        Clean,
		CheckedAt:    time.Now(),
		ResponseTime: responseTime,
	}

	if phishTankResp.Results.InDatabase {
		if phishTankResp.Results.Valid && phishTankResp.Results.Verified {
			result.Level = Blacklisted
			result.Description = fmt.Sprintf("Listed as verified phishing site (ID: %s)", phishTankResp.Results.PhishID)
			result.Score = 0.95
			result.Confidence = 0.95
			result.Categories = []string{"phishing"}
		} else if phishTankResp.Results.Valid {
			result.Level = Suspicious
			result.Description = fmt.Sprintf("Listed as potential phishing site (ID: %s)", phishTankResp.Results.PhishID)
			result.Score = 0.8
			result.Confidence = 0.8
			result.Categories = []string{"phishing"}
		} else {
			result.Level = Clean
			result.Description = "Previously reported but determined to be clean"
			result.Score = 0.1
			result.Confidence = 0.7
		}
	} else {
		result.Description = "Not found in PhishTank database"
		result.Score = 0.0
		result.Confidence = 0.5
	}

	return result, nil
}

// GetProviderName returns the provider name
func (p *PhishTankProvider) GetProviderName() string {
	return "PhishTank"
}
