package eventtrigger

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"net/url"
	"slices"
	"strings"
	"time"
	"unicode/utf8"
)

// The wake gate (EP-7, F-t3-047, slice W4-F T5) is a deterministic predicate over one fire's webhook payload. A trigger may
// carry a wake filter: a closed JSON predicate that Session Core validates and stores. Before the sweeper asks Control to
// authorise a fire of such a trigger, the gate reads the stored webhook event from integration-corev2 and evaluates the
// filter over it. A payload that does not match closes the gate, and the fire is refused. A payload that cannot be read
// leaves the fire pending for a later sweep, as an authority error does. The gate calls no model. It holds the payload in
// memory for one decision and never persists or logs it, because the body is untrusted data. Control still authorises
// every fire that passes, so a filter grants nothing.

// The closed wake filter schema, version 1. Session Core enforces the same bounds when it stores a filter. The gate checks
// them again, so a stored filter the schema does not allow closes the gate. maxWakeFilterStoredBytes is larger than the
// 4 KiB Session Core writes, because Postgres re-spaces stored JSON, which at most doubles it.
const (
	wakeFilterVersion        = 1
	maxWakeFilterStoredBytes = 16384
	maxWakeTokens            = 10
	maxWakeSubjects          = 5
	maxWakeFields            = 5
	maxWakePathSegments      = 5
	maxWakeTokenChars        = 100
	maxWakeValueChars        = 200
)

// The keys the gate sets over a provider body. Their names are camel case, and a field_equals path segment is only ever
// lowercase letters, digits and underscores, so a filter cannot name them. They are set after the body is copied, so the
// body cannot change what they hold.
const (
	wakeKeyEventType   = "eventType"
	wakeKeyProviderKey = "providerKey"
	wakeKeySubject     = "subject"
)

// WakeFilter is a stored wake filter after it has passed the closed schema. Within event_types, providers and
// subject_contains one listed value must match, and an empty list places no condition. Across the categories, and across
// the field_equals entries, every condition that is set must hold.
type WakeFilter struct {
	EventTypes      []string
	Providers       []string
	FieldEquals     []FieldEquals
	SubjectContains []string
}

// FieldEquals asserts that the string at Path, a dotted path into the provider body, equals Value exactly.
type FieldEquals struct {
	Path  string
	Value string
}

// wakeFilterWire is the stored filter as JSON. The decoder refuses every key the schema does not name.
type wakeFilterWire struct {
	Version         int               `json:"version"`
	EventTypes      []string          `json:"event_types"`
	Providers       []string          `json:"providers"`
	FieldEquals     []fieldEqualsWire `json:"field_equals"`
	SubjectContains []string          `json:"subject_contains"`
}

// fieldEqualsWire is one field_equals entry as JSON.
type fieldEqualsWire struct {
	Path  string `json:"path"`
	Value string `json:"value"`
}

// ParseWakeFilter checks a stored wake filter against the closed version 1 schema and returns it. A key the schema does not
// name, another version, and any bound the schema does not allow are all errors. The error never quotes the filter.
func ParseWakeFilter(raw json.RawMessage) (WakeFilter, error) {
	if len(raw) == 0 || len(raw) > maxWakeFilterStoredBytes {
		return WakeFilter{}, errors.New("wake filter is outside the schema's size")
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var wire wakeFilterWire
	if err := decoder.Decode(&wire); err != nil {
		return WakeFilter{}, errors.New("wake filter is not a JSON object in the closed schema")
	}
	if _, err := decoder.Token(); !errors.Is(err, io.EOF) {
		return WakeFilter{}, errors.New("wake filter has data after its object")
	}
	if wire.Version != wakeFilterVersion {
		return WakeFilter{}, errors.New("wake filter version is not 1")
	}
	eventTypes, err := wakeTokens(wire.EventTypes, maxWakeTokens, "event_types")
	if err != nil {
		return WakeFilter{}, err
	}
	providers, err := wakeTokens(wire.Providers, maxWakeTokens, "providers")
	if err != nil {
		return WakeFilter{}, err
	}
	subjects, err := wakeTokens(wire.SubjectContains, maxWakeSubjects, "subject_contains")
	if err != nil {
		return WakeFilter{}, err
	}
	if len(wire.FieldEquals) > maxWakeFields {
		return WakeFilter{}, fmt.Errorf("wake filter field_equals allows at most %d entries", maxWakeFields)
	}
	fields := make([]FieldEquals, 0, len(wire.FieldEquals))
	for _, entry := range wire.FieldEquals {
		if !validWakePath(entry.Path) {
			return WakeFilter{}, errors.New("wake filter field_equals paths are 1 to 5 segments of lowercase letters, digits and underscores")
		}
		if utf8.RuneCountInString(entry.Value) > maxWakeValueChars {
			return WakeFilter{}, fmt.Errorf("wake filter field_equals values are at most %d characters", maxWakeValueChars)
		}
		fields = append(fields, FieldEquals{Path: entry.Path, Value: entry.Value})
	}
	return WakeFilter{EventTypes: eventTypes, Providers: providers, FieldEquals: fields, SubjectContains: subjects}, nil
}

// wakeTokens checks one list of strings: at most limit entries, each non-blank and at most maxWakeTokenChars characters.
func wakeTokens(values []string, limit int, field string) ([]string, error) {
	if len(values) > limit {
		return nil, fmt.Errorf("wake filter %s allows at most %d entries", field, limit)
	}
	for _, value := range values {
		if strings.TrimSpace(value) == "" || utf8.RuneCountInString(value) > maxWakeTokenChars {
			return nil, fmt.Errorf("wake filter %s entries are 1 to %d characters", field, maxWakeTokenChars)
		}
	}
	return values, nil
}

// validWakePath reports whether a field_equals path is one to five dot-separated segments, each made of lowercase letters,
// digits and underscores.
func validWakePath(path string) bool {
	segments := strings.Split(path, ".")
	if len(segments) > maxWakePathSegments {
		return false
	}
	for _, segment := range segments {
		if segment == "" {
			return false
		}
		for i := 0; i < len(segment); i++ {
			c := segment[i]
			if (c < 'a' || c > 'z') && (c < '0' || c > '9') && c != '_' {
				return false
			}
		}
	}
	return true
}

// Evaluate reports whether one webhook event matches a wake filter, and why not. payload is the event as the gate builds
// it: the provider's JSON body, with the event's own eventType and providerKey set over it under their reserved keys. The
// body is read, never written, and it is trusted for nothing beyond the comparisons below. Evaluate is pure: it reads no
// clock, calls no model and does no I/O. reason is one fixed token, never payload content.
func Evaluate(filter WakeFilter, payload map[string]any) (bool, string) {
	if len(filter.EventTypes) > 0 {
		eventType, _ := payload[wakeKeyEventType].(string)
		if !slices.Contains(filter.EventTypes, eventType) {
			return false, "event_type_not_listed"
		}
	}
	if len(filter.Providers) > 0 {
		providerKey, _ := payload[wakeKeyProviderKey].(string)
		if !slices.Contains(filter.Providers, providerKey) {
			return false, "provider_not_listed"
		}
	}
	for _, field := range filter.FieldEquals {
		value, ok := lookupString(payload, field.Path)
		if !ok || value != field.Value {
			return false, "field_not_equal"
		}
	}
	if len(filter.SubjectContains) > 0 {
		subject, ok := payload[wakeKeySubject].(string)
		if !ok || !containsFold(subject, filter.SubjectContains) {
			return false, "subject_not_matched"
		}
	}
	return true, "matched"
}

// lookupString walks a dotted path through nested JSON objects and returns the string at its end. A missing segment, a
// non-object on the way and a value that is not a string are all no match.
func lookupString(payload map[string]any, path string) (string, bool) {
	segments := strings.Split(path, ".")
	current := payload
	for i, segment := range segments {
		value, ok := current[segment]
		if !ok {
			return "", false
		}
		if i == len(segments)-1 {
			text, ok := value.(string)
			return text, ok
		}
		next, ok := value.(map[string]any)
		if !ok {
			return "", false
		}
		current = next
	}
	return "", false
}

// containsFold reports whether subject contains any of needles, ignoring case.
func containsFold(subject string, needles []string) bool {
	folded := strings.ToLower(subject)
	for _, needle := range needles {
		if strings.Contains(folded, strings.ToLower(needle)) {
			return true
		}
	}
	return false
}

// wakeGate decides whether one pending fire's wake filter lets the fire through. A filter that does not match, or that is
// not in the closed schema, closes the gate: (false, nil). An error means the payload could not be read now, and the sweeper
// retries the fire.
type wakeGate interface {
	admit(ctx context.Context, fire Fire) (bool, error)
}

var _ wakeGate = payloadWakeGate{}

// payloadWakeGate is the wake gate over integration-corev2's stored webhook events.
type payloadWakeGate struct {
	source WebhookEventSource
}

// admit evaluates the fire's stored filter over its webhook event. The filter is parsed first, so a malformed filter never
// costs a read.
func (g payloadWakeGate) admit(ctx context.Context, fire Fire) (bool, error) {
	filter, err := ParseWakeFilter(fire.WakeFilter)
	if err != nil {
		slog.Warn("event trigger wake filter is outside the closed schema; the fire stays closed", "trigger_id", fire.TriggerID)
		return false, nil
	}
	if g.source == nil {
		return false, errors.New("wake gate has no webhook event source")
	}
	event, err := g.source.FetchWebhookEvent(ctx, fire.OrgID, fire.WebhookEventID)
	if err != nil {
		return false, err
	}
	open, reason := Evaluate(filter, event.view())
	if !open {
		slog.Info("event trigger wake gate closed", "trigger_id", fire.TriggerID, "fire_id", fire.ID, "reason", reason)
	}
	return open, nil
}

// WebhookEventSource reads one stored webhook event of an org. An error means the event could not be read now: the sweeper
// retries the fire, so an implementation must not keep a failure.
type WebhookEventSource interface {
	FetchWebhookEvent(ctx context.Context, orgID, webhookEventID string) (WebhookEventRecord, error)
}

// WebhookEventRecord is the part of integration-corev2's stored webhook event the gate reads: the provider, the event type
// and the provider's JSON body. The body is untrusted data.
type WebhookEventRecord struct {
	ProviderKey string         `json:"providerKey"`
	EventType   string         `json:"eventType"`
	Payload     map[string]any `json:"payload"`
}

// view is the map Evaluate reads: the provider body, with the event's own eventType and providerKey set over it under their
// reserved keys.
func (r WebhookEventRecord) view() map[string]any {
	view := make(map[string]any, len(r.Payload)+2)
	for key, value := range r.Payload {
		view[key] = value
	}
	view[wakeKeyEventType] = r.EventType
	view[wakeKeyProviderKey] = r.ProviderKey
	return view
}

// The bounds of one payload read. integration-corev2 accepts a webhook body of up to 4 MiB, so the response is read up to
// 8 MiB. The request gets the same five seconds the Control fire decision gets.
const (
	maxWebhookEventResponseBytes = 8 << 20
	webhookEventFetchTimeout     = 5 * time.Second
)

// IntegrationWebhookSource reads stored webhook events from integration-corev2's internal route. It is the only place a
// payload is fetched, and the internal key is sent in the internal header and never logged.
type IntegrationWebhookSource struct {
	baseURL string
	apiKey  string
	client  *http.Client
}

var _ WebhookEventSource = (*IntegrationWebhookSource)(nil)

// NewIntegrationWebhookSource builds the source over integration-corev2's base URL and internal API key. A source whose URL
// or key is blank reads nothing: every read fails, so a fire that needs the gate stays closed.
func NewIntegrationWebhookSource(baseURL, apiKey string) *IntegrationWebhookSource {
	return &IntegrationWebhookSource{
		baseURL: strings.TrimRight(strings.TrimSpace(baseURL), "/"),
		apiKey:  strings.TrimSpace(apiKey),
		client:  &http.Client{Timeout: webhookEventFetchTimeout, CheckRedirect: refuseRedirects},
	}
}

// refuseRedirects stops a fetch at the first redirect, so the internal key is never sent to another host.
func refuseRedirects(*http.Request, []*http.Request) error {
	return http.ErrUseLastResponse
}

// Configured reports whether the source has both a base URL and an internal key.
func (s *IntegrationWebhookSource) Configured() bool {
	return s != nil && s.baseURL != "" && s.apiKey != ""
}

// FetchWebhookEvent reads one webhook event of orgID. The response is decoded in memory for the gate and not kept: no field
// is logged or stored, and an error names only a status or a transport failure, never the URL or the body.
func (s *IntegrationWebhookSource) FetchWebhookEvent(ctx context.Context, orgID, webhookEventID string) (WebhookEventRecord, error) {
	if !s.Configured() {
		return WebhookEventRecord{}, errors.New("wake gate has no integration-corev2 URL or internal key")
	}
	endpoint := s.baseURL + "/internal/webhooks/events/" + url.PathEscape(webhookEventID) +
		"?organizationId=" + url.QueryEscape(orgID)
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return WebhookEventRecord{}, errors.New("build webhook event request")
	}
	request.Header.Set("X-Internal-API-Key", s.apiKey)
	request.Header.Set("Accept", "application/json")
	response, err := s.client.Do(request)
	if err != nil {
		var urlErr *url.Error
		if errors.As(err, &urlErr) {
			err = urlErr.Err
		}
		return WebhookEventRecord{}, fmt.Errorf("read webhook event: %w", err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return WebhookEventRecord{}, fmt.Errorf("read webhook event: integration-corev2 answered %d", response.StatusCode)
	}
	body, err := io.ReadAll(io.LimitReader(response.Body, maxWebhookEventResponseBytes+1))
	if err != nil {
		return WebhookEventRecord{}, errors.New("read webhook event body")
	}
	if len(body) > maxWebhookEventResponseBytes {
		return WebhookEventRecord{}, errors.New("webhook event is larger than the wake gate reads")
	}
	var envelope struct {
		Success bool `json:"success"`
		Data    struct {
			WebhookEvent *WebhookEventRecord `json:"webhookEvent"`
		} `json:"data"`
	}
	if err := json.Unmarshal(body, &envelope); err != nil {
		return WebhookEventRecord{}, errors.New("webhook event response is not JSON")
	}
	if !envelope.Success || envelope.Data.WebhookEvent == nil {
		return WebhookEventRecord{}, errors.New("webhook event response has no event")
	}
	return *envelope.Data.WebhookEvent, nil
}
