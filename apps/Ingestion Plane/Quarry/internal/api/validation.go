package api

import (
	"fmt"
	"net/url"
	"regexp"
	"strings"
)

var modulePattern = regexp.MustCompile(`^[a-zA-Z0-9\-_/]+$`)

const (
	defaultMapLimitMax      = 1000
	defaultSearchLimitMax   = 200
	defaultBatchURLsLimit   = 200
	defaultBatchWaitTimeout = 300
)

func validateAbsoluteHTTPURL(raw string) error {
	if strings.TrimSpace(raw) == "" {
		return fmt.Errorf("url is required")
	}
	parsed, err := url.ParseRequestURI(raw)
	if err != nil {
		return fmt.Errorf("url must be a valid absolute URL")
	}
	if parsed.Scheme != "http" && parsed.Scheme != "https" {
		return fmt.Errorf("url scheme must be http or https")
	}
	if parsed.Host == "" {
		return fmt.Errorf("url host is required")
	}
	return nil
}

func validateWebhookURL(raw string) error {
	if strings.TrimSpace(raw) == "" {
		return fmt.Errorf("webhook url is required")
	}
	return validateAbsoluteHTTPURL(raw)
}
