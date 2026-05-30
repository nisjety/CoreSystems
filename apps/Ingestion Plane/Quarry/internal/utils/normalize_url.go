package utils

import (
	"fmt"
	"net/url"
	"strings"
)

func NormalizeURL(rawURL string) (string, error) {
	// Add https:// if no protocol is specified
	if !strings.HasPrefix(rawURL, "http://") && !strings.HasPrefix(rawURL, "https://") {
		rawURL = "https://" + rawURL
	}

	// Parse and validate the URL
	parsedURL, err := url.Parse(rawURL)
	if err != nil {
		return "", err
	}

	// Check if host is present
	if parsedURL.Host == "" {
		return "", fmt.Errorf("missing host in URL")
	}

	return parsedURL.String(), nil
}
