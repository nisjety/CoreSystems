package trends

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"time"
)

// Endpoint is fixed inside the private dev Compose network, never user input.
type Client struct {
	enabled  bool
	http     *http.Client
	endpoint string
}

func New(environment string, enabled bool) *Client {
	return &Client{enabled: enabled && (environment == "local" || environment == "development" || environment == "test"), endpoint: "http://pytrends-dev:8000/report", http: &http.Client{Timeout: 28 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}}
}
func (c *Client) Collect(ctx context.Context, q Query) (Report, error) {
	if !Valid(q) {
		return Report{}, ErrInvalid
	}
	if !c.enabled {
		return Report{Query: q, Source: "pytrends_dev", Scale: "relative_0_100_per_request", Status: "disabled", ObservedAt: time.Now().UTC().Format(time.RFC3339), Rows: []Row{}}, nil
	}
	raw, _ := json.Marshal(q)
	req, err := http.NewRequestWithContext(ctx, "POST", c.endpoint, bytes.NewReader(raw))
	if err != nil {
		return Report{}, ErrUnavailable
	}
	req.Header.Set("Content-Type", "application/json")
	response, err := c.http.Do(req)
	if err != nil {
		return Report{}, ErrUnavailable
	}
	defer response.Body.Close()
	payload, err := io.ReadAll(io.LimitReader(response.Body, 131073))
	if err != nil || len(payload) > 131072 || response.StatusCode != 200 {
		return Report{}, ErrUnavailable
	}
	var report Report
	if json.Unmarshal(payload, &report) != nil || !ValidReport(report, q, time.Now()) {
		return Report{}, ErrUnavailable
	}
	return report, nil
}
