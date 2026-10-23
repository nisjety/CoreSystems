package siem

import (
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

const (
	destinationsPath = "/internal/orgs/siem-destinations"
	// An org holds at most three destinations and the list is every enabled
	// org's, so even a large deployment answers well inside this.
	maxDestinationsBody = 4 << 20
	orgCoreTimeout      = 10 * time.Second
)

// EventFilter narrows what one destination receives; an empty list means no
// restriction on that dimension. Both lists must match when both are set.
type EventFilter struct {
	Events []string `json:"events"`
	Planes []string `json:"planes"`
}

// Matches reports whether a row with this event name and plane is exported.
func (f EventFilter) Matches(event, plane string) bool {
	return listAllows(f.Events, event) && listAllows(f.Planes, plane)
}

func listAllows(allowed []string, value string) bool {
	if len(allowed) == 0 {
		return true
	}
	for _, candidate := range allowed {
		if candidate == value {
			return true
		}
	}
	return false
}

// Destination is one enabled SIEM destination as org-core serves it to the
// exporter. Secret is the unsealed delivery secret (HMAC key or HEC token): it
// lives in memory only. The type redacts itself in every formatted or encoded
// form, so a log line or error that includes a Destination cannot carry the
// secret or the URL.
type Destination struct {
	ID          string      `json:"id"`
	OrgID       string      `json:"org_id"`
	Kind        string      `json:"kind"`
	URL         string      `json:"url"`
	Secret      string      `json:"secret"`
	EventFilter EventFilter `json:"event_filter"`
	Revision    int64       `json:"revision"`
	UpdatedAt   time.Time   `json:"updated_at"`
}

func (d Destination) String() string {
	return fmt.Sprintf("destination{id=%s org=%s kind=%s}", d.ID, d.OrgID, d.Kind)
}

func (d Destination) GoString() string { return d.String() }

func (d Destination) MarshalJSON() ([]byte, error) {
	return json.Marshal(struct {
		ID    string `json:"id"`
		OrgID string `json:"org_id"`
		Kind  string `json:"kind"`
	}{d.ID, d.OrgID, d.Kind})
}

// OrgCoreClient reads destinations from org-core's internal route with the
// audit-core service credential (scope org:siem:read:any).
type OrgCoreClient struct {
	baseURL   string
	serviceID string
	token     string
	http      *http.Client
}

// NewOrgCoreClient builds the client. A nil httpClient gets a plain client with
// a timeout and no redirects: org-core is an internal service and the token
// must not follow a redirect anywhere.
func NewOrgCoreClient(baseURL, serviceID, token string, httpClient *http.Client) (*OrgCoreClient, error) {
	baseURL = strings.TrimRight(strings.TrimSpace(baseURL), "/")
	switch {
	case baseURL == "":
		return nil, errors.New("org-core url is required")
	case serviceID == "" || token == "":
		return nil, errors.New("org-core service identity and token are required")
	}
	if httpClient == nil {
		httpClient = &http.Client{
			Timeout:       orgCoreTimeout,
			CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse },
		}
	}
	return &OrgCoreClient{baseURL: baseURL, serviceID: serviceID, token: token, http: httpClient}, nil
}

// ListDestinations returns every enabled destination, or with a non-zero
// updatedSince only those changed at or after it. The exporter polls without
// the filter: the list is enabled-only, so a destination that was disabled or
// deleted simply stops appearing.
func (c *OrgCoreClient) ListDestinations(ctx context.Context, updatedSince time.Time) ([]Destination, error) {
	target := c.baseURL + destinationsPath
	if !updatedSince.IsZero() {
		target += "?" + url.Values{"updated_since": {updatedSince.UTC().Format(time.RFC3339)}}.Encode()
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, target, nil)
	if err != nil {
		return nil, errors.New("org-core destination request could not be built")
	}
	request.Header.Set("Accept", "application/json")
	request.Header.Set("X-Service-Id", c.serviceID)
	request.Header.Set("X-Service-Token", c.token)

	response, err := c.http.Do(request)
	if err != nil {
		// The transport error would embed the URL; the cause is enough.
		return nil, errors.New("org-core destination request failed")
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		_, _ = io.Copy(io.Discard, io.LimitReader(response.Body, maxResponseBytes))
		return nil, fmt.Errorf("org-core answered destination read with status %d", response.StatusCode)
	}
	var envelope struct {
		Data []Destination `json:"data"`
	}
	if err := json.NewDecoder(io.LimitReader(response.Body, maxDestinationsBody)).Decode(&envelope); err != nil {
		return nil, errors.New("org-core destination answer is not valid JSON")
	}
	return envelope.Data, nil
}
