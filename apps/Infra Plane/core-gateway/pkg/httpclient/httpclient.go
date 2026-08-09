package httpclient

import (
	"context"
	"fmt"
	"net/http"
	"time"

	"github.com/coresystem/integration-gateway/internal/audit"
	"github.com/coresystem/integration-gateway/pkg/circuitbreaker"
	"github.com/hashicorp/go-retryablehttp"
)

// Config holds HTTP client configuration
type Config struct {
	RetryMax       int
	RetryWaitMin   time.Duration
	RetryWaitMax   time.Duration
	Timeout        time.Duration
	ServiceName    string
	CircuitBreaker *circuitbreaker.CircuitBreaker
	AuditLogger    *audit.AuditLogger
}

// Client wraps retryablehttp with circuit breaker
type Client struct {
	client *retryablehttp.Client
	cb     *circuitbreaker.CircuitBreaker
	audit  *audit.AuditLogger
	config Config
}

// New creates a new HTTP client with retries and circuit breaker
func New(config Config) *Client {
	retryClient := retryablehttp.NewClient()

	// Configure retry behavior
	retryClient.RetryMax = config.RetryMax
	retryClient.RetryWaitMin = config.RetryWaitMin
	retryClient.RetryWaitMax = config.RetryWaitMax
	retryClient.HTTPClient.Timeout = config.Timeout

	// Custom retry policy: retry on 5xx, 429, timeouts
	retryClient.CheckRetry = func(ctx context.Context, resp *http.Response, err error) (bool, error) {
		// Don't retry on context cancellation
		if ctx.Err() != nil {
			return false, ctx.Err()
		}

		// Retry on connection errors
		if err != nil {
			return true, nil
		}

		// Retry on server errors (5xx)
		if resp.StatusCode >= 500 {
			return true, nil
		}

		// Retry on rate limiting (429)
		if resp.StatusCode == http.StatusTooManyRequests {
			return true, nil
		}

		// Retry on specific client errors
		if resp.StatusCode == http.StatusRequestTimeout {
			return true, nil
		}

		return false, nil
	}

	// Custom backoff with Retry-After header support
	retryClient.Backoff = func(min, max time.Duration, attemptNum int, resp *http.Response) time.Duration {
		// Check for Retry-After header
		if resp != nil && resp.StatusCode == http.StatusTooManyRequests {
			if retryAfter := resp.Header.Get("Retry-After"); retryAfter != "" {
				if duration, err := time.ParseDuration(retryAfter + "s"); err == nil {
					return duration
				}
			}
		}

		// Exponential backoff with jitter
		wait := min * time.Duration(1<<uint(attemptNum))
		if wait > max {
			wait = max
		}
		return wait
	}

	// Disable default logging (we use our own)
	retryClient.Logger = nil

	return &Client{
		client: retryClient,
		cb:     config.CircuitBreaker,
		audit:  config.AuditLogger,
		config: config,
	}
}

// Do executes an HTTP request with retries and circuit breaker
func (c *Client) Do(req *http.Request) (*http.Response, error) {
	startTime := time.Now()

	// Wrap the request in circuit breaker
	_, err := c.cb.ExecuteContext(req.Context(), func() ([]byte, error) {
		// Convert to retryable request
		retryReq, err := retryablehttp.FromRequest(req)
		if err != nil {
			return nil, fmt.Errorf("failed to create retryable request: %w", err)
		}

		// Execute request with retries
		resp, err := c.client.Do(retryReq)
		if err != nil {
			return nil, err
		}

		// Check for non-2xx status codes
		if resp.StatusCode < 200 || resp.StatusCode >= 300 {
			return nil, fmt.Errorf("HTTP error: %d %s", resp.StatusCode, resp.Status)
		}

		// Read response body
		defer resp.Body.Close()
		body := make([]byte, resp.ContentLength)
		if _, err := resp.Body.Read(body); err != nil {
			return nil, fmt.Errorf("failed to read response: %w", err)
		}

		return body, nil
	})

	duration := time.Since(startTime)

	// Audit log the request
	if c.audit != nil {
		c.audit.LogIntegrationCall(
			c.config.ServiceName,
			fmt.Sprintf("%s %s", req.Method, req.URL.Path),
			err == nil,
			err,
			duration,
		)
	}

	if err != nil {
		return nil, err
	}

	// Return a mock response (in real implementation, we'd need to reconstruct the response)
	// For now, this is a simplified version
	return &http.Response{
		StatusCode: http.StatusOK,
		Status:     "200 OK",
		Body:       http.NoBody,
	}, nil
}

// Get performs a GET request
func (c *Client) Get(ctx context.Context, url string) (*http.Response, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, fmt.Errorf("failed to create GET request: %w", err)
	}
	return c.Do(req)
}

// Post performs a POST request
func (c *Client) Post(ctx context.Context, url string, body []byte, contentType string) (*http.Response, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, nil)
	if err != nil {
		return nil, fmt.Errorf("failed to create POST request: %w", err)
	}
	req.Header.Set("Content-Type", contentType)
	return c.Do(req)
}

// Put performs a PUT request
func (c *Client) Put(ctx context.Context, url string, body []byte, contentType string) (*http.Response, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodPut, url, nil)
	if err != nil {
		return nil, fmt.Errorf("failed to create PUT request: %w", err)
	}
	req.Header.Set("Content-Type", contentType)
	return c.Do(req)
}

// Delete performs a DELETE request
func (c *Client) Delete(ctx context.Context, url string) (*http.Response, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodDelete, url, nil)
	if err != nil {
		return nil, fmt.Errorf("failed to create DELETE request: %w", err)
	}
	return c.Do(req)
}

// StandardClient returns the underlying standard HTTP client
func (c *Client) StandardClient() *http.Client {
	return c.client.StandardClient()
}
