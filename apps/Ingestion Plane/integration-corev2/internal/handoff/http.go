package handoff

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

const defaultTimeout = 10 * time.Second

var ErrNotConfigured = errors.New("handoff client is not configured")

type ServiceHTTPError struct {
	Service    string
	StatusCode int
}

func (e *ServiceHTTPError) Error() string {
	if e == nil {
		return ""
	}
	return fmt.Sprintf("%s returned HTTP %d", e.Service, e.StatusCode)
}

type httpClient interface {
	Do(*http.Request) (*http.Response, error)
}

type serviceError struct {
	Code string `json:"code"`
}

type serviceEnvelope[T any] struct {
	Success bool          `json:"success"`
	Data    T             `json:"data"`
	Error   *serviceError `json:"error,omitempty"`
}

func withDefaultHTTPClient(client *http.Client) httpClient {
	if client != nil {
		return client
	}
	return &http.Client{Timeout: defaultTimeout}
}

func normalizeBaseURL(input string) string {
	return strings.TrimRight(strings.TrimSpace(input), "/")
}

func normalizeHeader(input, fallback string) string {
	if trimmed := strings.TrimSpace(input); trimmed != "" {
		return trimmed
	}
	return fallback
}

func pathJoin(baseURL string, parts ...string) string {
	out := strings.TrimRight(baseURL, "/")
	for _, part := range parts {
		out += "/" + strings.Trim(url.PathEscape(strings.TrimSpace(part)), "/")
	}
	return out
}

func jsonRequest(ctx context.Context, method, endpoint string, body any) (*http.Request, error) {
	var reader io.Reader
	if body != nil {
		encoded, err := json.Marshal(body)
		if err != nil {
			return nil, fmt.Errorf("marshal handoff request: %w", err)
		}
		reader = bytes.NewReader(encoded)
	}
	req, err := http.NewRequestWithContext(ctx, method, endpoint, reader)
	if err != nil {
		return nil, err
	}
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	return req, nil
}

func decodeEnvelope[T any](serviceName string, resp *http.Response) (T, error) {
	defer resp.Body.Close()
	var zero T
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return zero, &ServiceHTTPError{Service: serviceName, StatusCode: resp.StatusCode}
	}
	var envelope serviceEnvelope[T]
	if err := json.NewDecoder(resp.Body).Decode(&envelope); err != nil {
		return zero, fmt.Errorf("%s returned invalid JSON: %w", serviceName, err)
	}
	if envelope.Error != nil && strings.TrimSpace(envelope.Error.Code) != "" {
		return zero, fmt.Errorf("%s returned error %q", serviceName, strings.TrimSpace(envelope.Error.Code))
	}
	if !envelope.Success {
		return zero, fmt.Errorf("%s returned an error envelope", serviceName)
	}
	return envelope.Data, nil
}

func decodeJSONOrEnvelope[T any](serviceName string, resp *http.Response) (T, error) {
	defer resp.Body.Close()
	var zero T
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return zero, &ServiceHTTPError{Service: serviceName, StatusCode: resp.StatusCode}
	}
	var raw map[string]json.RawMessage
	if err := json.NewDecoder(resp.Body).Decode(&raw); err != nil {
		return zero, fmt.Errorf("%s returned invalid JSON: %w", serviceName, err)
	}
	payload := raw
	if data, ok := raw["data"]; ok {
		if err, hasError := raw["error"]; hasError && len(err) > 0 && string(err) != "null" {
			return zero, fmt.Errorf("%s returned an error envelope", serviceName)
		}
		var dataPayload map[string]json.RawMessage
		if err := json.Unmarshal(data, &dataPayload); err != nil {
			return zero, fmt.Errorf("%s returned invalid data payload: %w", serviceName, err)
		}
		payload = dataPayload
	}
	encoded, err := json.Marshal(payload)
	if err != nil {
		return zero, err
	}
	var out T
	if err := json.Unmarshal(encoded, &out); err != nil {
		return zero, fmt.Errorf("%s returned unexpected payload: %w", serviceName, err)
	}
	return out, nil
}
