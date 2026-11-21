package browsersites

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"log"
	"net/http"
	"net/url"
	"strings"
	"time"
)

// ErrFloorUnavailable means the Space's browser floor could not be read: user-core did not
// answer, answered something other than 200, or answered a body that is not the expected
// answer. ResolveForSpace reads it as not entitled, so the Space is refused every host.
var ErrFloorUnavailable = errors.New("space browser floor is unavailable")

// SpaceFloorReader reads one Space's browser_entitled answer (DEC-19 D9). user-core owns the
// Space and its answer, so org-core reads it through the internal browser-floor route and never
// through user-core's tables. An error leaves the floor unknown, and ResolveForSpace refuses.
type SpaceFloorReader interface {
	BrowserFloor(ctx context.Context, orgID, spaceID string) (bool, error)
}

const (
	// spaceFloorTimeout bounds one floor read. It is shorter than the resolve's own bound, so a
	// slow user-core is refused instead of holding the broker's grant mint.
	spaceFloorTimeout = 2 * time.Second
	// spaceFloorBodyLimit bounds the answer body. The answer is one small object.
	spaceFloorBodyLimit = 4 << 10
	// spaceFloorServiceID is org-core's identity on the user-core service hop.
	spaceFloorServiceID = "org-core"
)

// userCoreFloorReader reads the floor from user-core under org-core's own service identity. It
// keeps no cache: a Space that loses its entitlement is refused on the next resolve.
type userCoreFloorReader struct {
	baseURL string
	token   string
	client  *http.Client
}

// NewUserCoreFloorReader returns the reader that GETs
// {baseURL}/api/v1/internal/spaces/{space}/browser-floor?org_id={org} with org-core's service id
// and token. It returns nil when the base URL or the token is blank, and a nil reader refuses every
// host of a Space-scoped resolve. The client is copied, never modified: the copy never follows a
// redirect and waits at most spaceFloorTimeout. A nil client gets a fresh one.
func NewUserCoreFloorReader(baseURL, serviceToken string, client *http.Client) SpaceFloorReader {
	baseURL = strings.TrimRight(strings.TrimSpace(baseURL), "/")
	serviceToken = strings.TrimSpace(serviceToken)
	if baseURL == "" || serviceToken == "" {
		return nil
	}
	floorClient := &http.Client{}
	if client != nil {
		copied := *client
		floorClient = &copied
	}
	if floorClient.Timeout <= 0 || floorClient.Timeout > spaceFloorTimeout {
		floorClient.Timeout = spaceFloorTimeout
	}
	floorClient.CheckRedirect = func(*http.Request, []*http.Request) error {
		return http.ErrUseLastResponse
	}
	return &userCoreFloorReader{baseURL: baseURL, token: serviceToken, client: floorClient}
}

// BrowserFloor reads whether the Space is entitled to the agent browser. A transport error, a status
// other than 200 and a body that is not exactly the expected answer are all ErrFloorUnavailable. The
// log names the organisation and the status only, never the body.
func (r *userCoreFloorReader) BrowserFloor(ctx context.Context, orgID, spaceID string) (bool, error) {
	ctx, cancel := context.WithTimeout(ctx, spaceFloorTimeout)
	defer cancel()
	endpoint := r.baseURL + "/api/v1/internal/spaces/" + url.PathEscape(spaceID) + "/browser-floor?" +
		url.Values{"org_id": {orgID}}.Encode()
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return false, ErrFloorUnavailable
	}
	request.Header.Set("X-Service-Id", spaceFloorServiceID)
	request.Header.Set("X-Service-Token", r.token)
	request.Header.Set("Accept", "application/json")
	response, err := r.client.Do(request)
	if err != nil {
		log.Printf("browser sites: space floor read for org=%s did not answer", orgID)
		return false, ErrFloorUnavailable
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		log.Printf("browser sites: space floor read for org=%s answered status=%d", orgID, response.StatusCode)
		return false, ErrFloorUnavailable
	}
	entitled, ok := decodeSpaceFloor(response.Body, spaceID)
	if !ok {
		log.Printf("browser sites: space floor read for org=%s answered an unreadable body, status=%d", orgID, response.StatusCode)
		return false, ErrFloorUnavailable
	}
	return entitled, nil
}

// spaceFloorAnswer is user-core's answer: {"data":{"space_ref":"...","browser_entitled":true}}.
type spaceFloorAnswer struct {
	Data struct {
		SpaceRef        string `json:"space_ref"`
		BrowserEntitled *bool  `json:"browser_entitled"`
	} `json:"data"`
}

// decodeSpaceFloor reads the answer strictly: one JSON object of the known shape, with no unknown
// field, no trailing data, within the size limit, and naming the Space that was asked for. It
// reports false for anything else, which the caller reads as unknown.
func decodeSpaceFloor(body io.Reader, spaceID string) (bool, bool) {
	raw, err := io.ReadAll(io.LimitReader(body, spaceFloorBodyLimit+1))
	if err != nil || len(raw) > spaceFloorBodyLimit {
		return false, false
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var answer spaceFloorAnswer
	if err := decoder.Decode(&answer); err != nil {
		return false, false
	}
	if _, err := decoder.Token(); err != io.EOF {
		return false, false
	}
	if answer.Data.BrowserEntitled == nil || answer.Data.SpaceRef != spaceID {
		return false, false
	}
	return *answer.Data.BrowserEntitled, true
}
