// Package activities implements Temporal activities that bridge workflows
// to Quarry Runtime (page execution) and Quarry Control (checkpoints/events).
//
// All activities translate transport and HTTP errors into classified
// [errs.Error] values and return [temporal.ApplicationError] results so that
// workflow-side retry policies honor Quarry's non-retryable categories.
package activities

import (
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/controlauth"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/errs"
)

// Config holds endpoint URLs and bearer tokens for the upstream services.
type Config struct {
	RuntimeBaseURL    string
	ControlBaseURL    string
	RuntimeAuthToken  string
	ControlAuthToken  string
	ControlHMACSecret string
	// EdgeBaseURL + EdgeAuthToken target quarry-edge's internal change
	// endpoint (/v1/internal/change/record). The baseline+diff store is
	// Rust and edge-local, so the Go change-monitor activity persists
	// through the edge rather than calling the store in-process.
	EdgeBaseURL   string
	EdgeAuthToken string
	// EdgeRunSecret is the shared HMAC secret (env QUARRY_INTERNAL_SECRET)
	// used to org-bind the /v1/internal/run_page call. When non-empty the
	// orchestrator stamps an X-Quarry-Run-Sig header over org_id+url so a
	// leaked runtime bearer token can't be replayed against another tenant's
	// org_id. Empty = no binding (edge falls back to runtime-token-only).
	EdgeRunSecret string
	HTTPTimeout   time.Duration
}

// Activities wires HTTP calls into Temporal activity methods.
type Activities struct {
	cfg             Config
	http            *http.Client
	DurableFrontier bool
}

// New returns an Activities instance with a configured HTTP client.
func New(cfg Config) *Activities {
	timeout := cfg.HTTPTimeout
	if timeout <= 0 {
		timeout = 2 * time.Minute
	}
	return &Activities{
		cfg:             cfg,
		http:            &http.Client{Timeout: timeout},
		DurableFrontier: cfg.EdgeBaseURL != "" && cfg.EdgeAuthToken != "" && cfg.EdgeRunSecret != "",
	}
}

// FrontierEnqueueInput/FrontierPopInput are the narrow Go/Temporal wire
// bridge to Quarry's Rust-owned PostgresRequestQueue. Depth lives in payload
// so the queue remains generic and the workflow can resume after a worker
// restart without retaining a frontier slice in Temporal history.
type FrontierEnqueueInput struct {
	Queue     string `json:"queue"`
	OrgID     string `json:"org_id"`
	RequestID string `json:"request_id"`
	URL       string `json:"url"`
	Depth     uint32 `json:"depth"`
}

type FrontierPopInput struct {
	Queue string `json:"queue"`
	OrgID string `json:"org_id"`
}

type FrontierQueueItem struct {
	RequestID string         `json:"request_id"`
	URL       string         `json:"url"`
	Payload   map[string]any `json:"payload"`
	Attempt   uint32         `json:"attempt"`
}

type FrontierAckInput struct {
	Queue     string `json:"queue"`
	OrgID     string `json:"org_id"`
	RequestID string `json:"request_id"`
}

func (a *Activities) FrontierEnqueue(ctx context.Context, in FrontierEnqueueInput) error {
	body, err := json.Marshal(map[string]any{
		"org_id": in.OrgID, "request_id": in.RequestID, "url": in.URL,
		"priority": "default", "payload": map[string]any{"depth": in.Depth},
	})
	if err != nil {
		return errs.New(errs.CategoryValidation, "activities.FrontierEnqueue", err).Temporal()
	}
	return a.frontierCall(ctx, http.MethodPost, "/v1/internal/queues/"+in.Queue+"/enqueue", in.OrgID, in.Queue, body, nil)
}

func (a *Activities) FrontierPop(ctx context.Context, in FrontierPopInput) (*FrontierQueueItem, error) {
	body, err := json.Marshal(map[string]string{"org_id": in.OrgID})
	if err != nil {
		return nil, errs.New(errs.CategoryValidation, "activities.FrontierPop", err).Temporal()
	}
	var envelope quarrycontracts.RESTEnvelope[*FrontierQueueItem]
	if err := a.frontierCall(ctx, http.MethodPost, "/v1/internal/queues/"+in.Queue+"/pop", in.OrgID, in.Queue, body, &envelope); err != nil {
		return nil, err
	}
	if envelope.Data == nil || *envelope.Data == nil {
		return nil, nil
	}
	return *envelope.Data, nil
}

func (a *Activities) FrontierAck(ctx context.Context, in FrontierAckInput) error {
	body, err := json.Marshal(map[string]string{"org_id": in.OrgID, "request_id": in.RequestID})
	if err != nil {
		return errs.New(errs.CategoryValidation, "activities.FrontierAck", err).Temporal()
	}
	return a.frontierCall(ctx, http.MethodPut, "/v1/internal/queues/"+in.Queue+"/ack", in.OrgID, in.Queue, body, nil)
}

func (a *Activities) frontierCall(ctx context.Context, method, path, orgID, queue string, body []byte, out any) error {
	const op = "activities.FrontierCall"
	req, err := http.NewRequestWithContext(ctx, method, strings.TrimRight(a.cfg.EdgeBaseURL, "/")+path, bytes.NewReader(body))
	if err != nil {
		return errs.New(errs.CategoryValidation, op, err).Temporal()
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Quarry-Queue-Sig", runBindingSig(a.cfg.EdgeRunSecret, orgID, queue))
	setAuth(req, a.cfg.EdgeAuthToken)
	resp, err := a.http.Do(req)
	if err != nil {
		if errors.Is(err, context.Canceled) {
			return err
		}
		return errs.New(errs.CategoryNetwork, op, err).Temporal()
	}
	defer resp.Body.Close()
	respBody, _ := io.ReadAll(resp.Body)
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return errs.FromHTTPStatus(op, resp.StatusCode, respBody).Temporal()
	}
	if out != nil {
		if err := json.Unmarshal(respBody, out); err != nil {
			return errs.New(errs.CategoryValidation, op, fmt.Errorf("decode frontier response: %w", err)).Temporal()
		}
	}
	return nil
}

// RunPageInput is the input for a single page execution.
type RunPageInput struct {
	RunID string `json:"run_id"`
	URL   string `json:"url"`
	// OrgID is the originating tenant, stamped by the workflow from the
	// schedule/job memo. It travels in the run_page body (the edge reads
	// org_id from the body, not a JWT) AND is HMAC-bound via the
	// X-Quarry-Run-Sig header so it can't be forged with a leaked bearer.
	OrgID string `json:"org_id"`
	// UserID is the verified-JWT initiator, threaded from the workflow input.
	// It travels in the run_page body; the edge forwards it as x-user-id on
	// ingest so crawled docs are owner-stamped private (private-by-default).
	// Empty = system/scheduled run → org-visible (legacy).
	UserID string `json:"user_id,omitempty"`
	// Ingest (Phase 2 selective ingest): true = the edge persists+embeds this
	// page into the Data Plane; absent/false = working-set only (default NEVER).
	Ingest bool `json:"ingest,omitempty"`
}

// RunPageResult is returned by the runtime after a successful page execution.
//
// ContentType, Title, and Branding are best-effort fields the runtime
// populates for HTML responses. They're optional for backward
// compatibility — older runtime builds simply omit them and consumers
// must degrade gracefully (e.g. the verevon wizard falls back to URL-
// based snippet kind detection when ContentType is empty).
type RunPageResult struct {
	RunID       string         `json:"run_id"`
	Status      uint16         `json:"status"`
	Fingerprint string         `json:"fingerprint"`
	Links       []string       `json:"links"`
	ContentType string         `json:"content_type,omitempty"`
	Title       string         `json:"title,omitempty"`
	Branding    map[string]any `json:"branding,omitempty"`
	// Post-transform extraction (quarry-edge InternalRunPageResult, Rust).
	// Empty TitleSource means the runtime predates the extraction or the
	// response was not HTML; the workflow then emits no page_extracted.
	DisplayTitle string `json:"display_title,omitempty"`
	TitleSource  string `json:"title_source,omitempty"`
	Excerpt      string `json:"excerpt,omitempty"`
	Summary      string `json:"summary,omitempty"`
	WordCount    uint64 `json:"word_count,omitempty"`
	Lang         string `json:"lang,omitempty"`
	Driver       string `json:"driver,omitempty"`
}

// RunPage executes a single page via Quarry Runtime.
func (a *Activities) RunPage(ctx context.Context, in RunPageInput) (RunPageResult, error) {
	const op = "activities.RunPage"
	body, err := json.Marshal(map[string]any{"url": in.URL, "org_id": in.OrgID, "user_id": in.UserID, "ingest": in.Ingest})
	if err != nil {
		return RunPageResult{}, errs.New(errs.CategoryValidation, op, err).Temporal()
	}

	url := strings.TrimRight(a.cfg.RuntimeBaseURL, "/") + "/v1/internal/run_page"
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return RunPageResult{}, errs.New(errs.CategoryValidation, op, err).Temporal()
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Idempotency-Key", idempotencyKey(in.RunID, in.URL))
	setAuth(req, a.cfg.RuntimeAuthToken)
	// HMAC org-binding: a leaked runtime bearer can't forge a tenant's
	// org_id. Sign over org_id+"\n"+url (matching the edge's
	// verify_run_binding byte-for-byte). Skipped when no shared secret is
	// configured — the edge then falls back to runtime-token-only.
	if a.cfg.EdgeRunSecret != "" {
		sig := runBindingSig(a.cfg.EdgeRunSecret, in.OrgID, in.URL)
		req.Header.Set("X-Quarry-Run-Sig", sig)
	}

	resp, err := a.http.Do(req)
	if err != nil {
		// Distinguish ctx cancellation from genuine network failure.
		// Temporal cancels activities when the parent workflow is
		// cancelled; treating that as a retryable network error makes
		// Temporal reschedule the cancelled activity. Re-raising the
		// raw context.Canceled lets Temporal honor the cancellation.
		if errors.Is(err, context.Canceled) {
			return RunPageResult{}, err
		}
		cat := errs.CategoryNetwork
		if errors.Is(err, context.DeadlineExceeded) {
			cat = errs.CategoryTimeout
		}
		return RunPageResult{}, errs.New(cat, op, err).Temporal()
	}
	defer resp.Body.Close()

	respBody, _ := io.ReadAll(resp.Body)
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return RunPageResult{}, errs.FromHTTPStatus(op, resp.StatusCode, respBody).Temporal()
	}

	var out RunPageResult
	if err := json.Unmarshal(respBody, &out); err != nil {
		return RunPageResult{}, errs.New(errs.CategoryValidation, op, fmt.Errorf("decode response: %w", err)).Temporal()
	}
	out.RunID = in.RunID
	return out, nil
}

// CheckpointInput is the input for a run checkpoint.
type CheckpointInput struct {
	RunID    string `json:"run_id"`
	Visited  uint32 `json:"visited"`
	Frontier uint32 `json:"frontier"`
}

// Checkpoint persists a run progress checkpoint via Quarry Control.
func (a *Activities) Checkpoint(ctx context.Context, in CheckpointInput) error {
	const op = "activities.Checkpoint"
	body, err := json.Marshal(in)
	if err != nil {
		return errs.New(errs.CategoryValidation, op, err).Temporal()
	}

	url := fmt.Sprintf("%s/v1/runs/%s/checkpoints", strings.TrimRight(a.cfg.ControlBaseURL, "/"), in.RunID)
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return errs.New(errs.CategoryValidation, op, err).Temporal()
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Idempotency-Key", idempotencyKey(in.RunID, strconv.FormatUint(uint64(in.Visited), 10), strconv.FormatUint(uint64(in.Frontier), 10)))
	setAuth(req, a.cfg.ControlAuthToken)
	if err := controlauth.Sign(req, body, a.cfg.ControlHMACSecret); err != nil {
		return errs.New(errs.CategoryNetwork, op, err).Temporal()
	}

	resp, err := a.http.Do(req)
	if err != nil {
		cat := errs.CategoryNetwork
		if errors.Is(err, context.DeadlineExceeded) {
			cat = errs.CategoryTimeout
		}
		return errs.New(cat, op, err).Temporal()
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		respBody, _ := io.ReadAll(resp.Body)
		return errs.FromHTTPStatus(op, resp.StatusCode, respBody).Temporal()
	}
	return nil
}

// EmitEvent publishes a Quarry event via Quarry Control using the event's own
// IdempotencyKey as the HTTP Idempotency-Key header.
//
// Control's `POST /v1/runs/{id}/events` handler accepts a JSON ARRAY of
// events (batch ingest contract — see resources.go MountEvents). We
// wrap the single event in a one-element slice so the wire shape
// matches; control returns 400 on a bare object.
func (a *Activities) EmitEvent(ctx context.Context, runID string, event quarrycontracts.Event) error {
	const op = "activities.EmitEvent"
	body, err := json.Marshal([]quarrycontracts.Event{event})
	if err != nil {
		return errs.New(errs.CategoryValidation, op, err).Temporal()
	}

	url := fmt.Sprintf("%s/v1/runs/%s/events", strings.TrimRight(a.cfg.ControlBaseURL, "/"), runID)
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return errs.New(errs.CategoryValidation, op, err).Temporal()
	}
	req.Header.Set("Content-Type", "application/json")
	if event.IdempotencyKey != "" {
		req.Header.Set("Idempotency-Key", event.IdempotencyKey)
	}
	setAuth(req, a.cfg.ControlAuthToken)
	if err := controlauth.Sign(req, body, a.cfg.ControlHMACSecret); err != nil {
		return errs.New(errs.CategoryNetwork, op, err).Temporal()
	}

	resp, err := a.http.Do(req)
	if err != nil {
		cat := errs.CategoryNetwork
		if errors.Is(err, context.DeadlineExceeded) {
			cat = errs.CategoryTimeout
		}
		return errs.New(cat, op, err).Temporal()
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		respBody, _ := io.ReadAll(resp.Body)
		return errs.FromHTTPStatus(op, resp.StatusCode, respBody).Temporal()
	}
	return nil
}

// CheckChangeInput drives a single change-monitor probe.
type CheckChangeInput struct {
	RunID string `json:"run_id"`
	OrgID string `json:"org_id"`
	URL   string `json:"url"`
}

// CheckChangeResult is the outcome of comparing a fresh fetch against the
// org's stored baseline for the URL. Status is one of new|unchanged|changed.
type CheckChangeResult struct {
	Status          string `json:"status"`
	Changed         bool   `json:"changed"`
	Fingerprint     string `json:"fingerprint"`
	PrevFingerprint string `json:"prev_fingerprint,omitempty"`
	BaselineID      string `json:"baseline_id,omitempty"`
	DiffID          string `json:"diff_id,omitempty"`
}

// changeRecordRequest is the body POSTed to quarry-edge's internal
// /v1/internal/change/record endpoint. The edge owns the Rust baseline
// store, so it runs compare→save_baseline→(on change)create_diff in-process
// and returns the populated result.
type changeRecordRequest struct {
	OrgID            string `json:"org_id"`
	URL              string `json:"url"`
	FreshFingerprint string `json:"fresh_fingerprint"`
	RunID            string `json:"run_id"`
}

// CheckChange fetches the URL fresh (via the runtime), then asks the edge to
// compare it against the stored baseline and persist a new baseline (+ diff
// on change). Returns whether the page changed plus the persisted ids.
//
// Two upstream calls: (1) runtime /v1/internal/run_page for the fresh
// fingerprint (the runtime owns page execution; it has no change endpoint),
// (2) edge /v1/internal/change/record for the compare+persist (the edge owns
// the Rust baseline store). Go can't touch the Rust in-process store, so the
// persisting step MUST route through the edge.
func (a *Activities) CheckChange(ctx context.Context, in CheckChangeInput) (CheckChangeResult, error) {
	const op = "activities.CheckChange"

	// 1) Fresh fetch via the runtime — reuse RunPage's classified HTTP path.
	// Pass OrgID so RunPage stamps the HMAC org-binding (the edge enforces
	// it on /v1/internal/run_page).
	page, err := a.RunPage(ctx, RunPageInput{RunID: in.RunID, URL: in.URL, OrgID: in.OrgID})
	if err != nil {
		return CheckChangeResult{}, err
	}
	if page.Fingerprint == "" {
		return CheckChangeResult{}, errs.New(errs.CategoryValidation, op, fmt.Errorf("runtime returned empty fingerprint for %s", in.URL)).Temporal()
	}

	// 2) Compare + persist via the edge's Rust-owned baseline store.
	body, err := json.Marshal(changeRecordRequest{
		OrgID:            in.OrgID,
		URL:              in.URL,
		FreshFingerprint: page.Fingerprint,
		RunID:            in.RunID,
	})
	if err != nil {
		return CheckChangeResult{}, errs.New(errs.CategoryValidation, op, err).Temporal()
	}

	url := strings.TrimRight(a.cfg.EdgeBaseURL, "/") + "/v1/internal/change/record"
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return CheckChangeResult{}, errs.New(errs.CategoryValidation, op, err).Temporal()
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Idempotency-Key", idempotencyKey(in.RunID, in.URL, page.Fingerprint))
	setAuth(req, a.cfg.EdgeAuthToken)

	resp, err := a.http.Do(req)
	if err != nil {
		if errors.Is(err, context.Canceled) {
			return CheckChangeResult{}, err
		}
		cat := errs.CategoryNetwork
		if errors.Is(err, context.DeadlineExceeded) {
			cat = errs.CategoryTimeout
		}
		return CheckChangeResult{}, errs.New(cat, op, err).Temporal()
	}
	defer resp.Body.Close()

	respBody, _ := io.ReadAll(resp.Body)
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return CheckChangeResult{}, errs.FromHTTPStatus(op, resp.StatusCode, respBody).Temporal()
	}

	var out CheckChangeResult
	if err := json.Unmarshal(respBody, &out); err != nil {
		return CheckChangeResult{}, errs.New(errs.CategoryValidation, op, fmt.Errorf("decode change record: %w", err)).Temporal()
	}
	if out.Fingerprint == "" {
		out.Fingerprint = page.Fingerprint
	}
	return out, nil
}

// setAuth attaches a bearer token if non-empty.
func setAuth(req *http.Request, token string) {
	if token != "" {
		req.Header.Set("Authorization", "Bearer "+token)
	}
}

// idempotencyKey returns a deterministic SHA-256 hex digest of the joined parts.
func idempotencyKey(parts ...string) string {
	sum := sha256.Sum256([]byte(strings.Join(parts, ":")))
	return hex.EncodeToString(sum[:])
}

// runBindingSig computes the X-Quarry-Run-Sig value: lowercase-hex
// HMAC-SHA256(secret, org_id+"\n"+url). The canonical string MUST match
// the edge's InternalSigner::verify_run_binding byte-for-byte (org_id, a
// single 0x0A newline, then the raw url; NO trailing newline).
func runBindingSig(secret, orgID, url string) string {
	mac := hmac.New(sha256.New, []byte(secret))
	mac.Write([]byte(orgID + "\n" + url))
	return hex.EncodeToString(mac.Sum(nil))
}
