package gates

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"time"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/authctx"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
)

// ParityContract is the wire contract of the keyword-index parity endpoint that
// quickwit-adapter-rs serves (CR-T9a) and this service consumes. The contract is
// pinned by testdata/index-parity-v1/ (a response golden and its sha256), which
// the adapter vendors byte for byte; scripts/check-parity-contract.sh is the
// repo-level drift check.
//
// Why an endpoint and not a query: data-quality-go has no access to Quickwit (it
// has no client, no credential and no network path to it). The adapter owns the
// index, so the adapter measures it and this service judges the measurement.
const ParityContract = "index-parity-v1"

// ParityReport is the response body of the parity endpoint.
//
// Definitions (the golden's README is the authority):
//
//   - postgres.eligible_units: units with embedding_status 'done', whose document
//     is not deleted and is classified internal, public or sensitive: the exact
//     population the adapter indexes.
//   - postgres.eligible_with_context: of those, units whose stored chunk_context
//     is not blank.
//   - quickwit.documents: knowledge_unit documents of the organization in the
//     index, as the index counts them (a delete is eventual, so this can exceed
//     the number of distinct units).
//   - quickwit.distinct_units: the same, de-duplicated by knowledge_id (this
//     Quickwit refuses the cardinality aggregation).
//   - quickwit.with_context_body: distinct units whose context_body is not blank.
//   - difference.*: set differences computed by the adapter over knowledge ids.
type ParityReport struct {
	Contract  string `json:"contract"`
	OrgID     string `json:"org_id"`
	CheckedAt string `json:"checked_at"`
	Postgres  struct {
		EligibleUnits       int64 `json:"eligible_units"`
		EligibleWithContext int64 `json:"eligible_with_context"`
	} `json:"postgres"`
	Quickwit struct {
		Documents       int64 `json:"documents"`
		DistinctUnits   int64 `json:"distinct_units"`
		WithContextBody int64 `json:"with_context_body"`
	} `json:"quickwit"`
	Difference struct {
		// MissingUnits are eligible in Postgres and absent from the index.
		MissingUnits int64 `json:"missing_units"`
		// UnexpectedUnits are in the index and not eligible in Postgres (stale).
		UnexpectedUnits int64 `json:"unexpected_units"`
		// ContextBodyMissing are indexed units whose Postgres context is not blank
		// and whose context_body is blank or absent.
		ContextBodyMissing int64 `json:"context_body_missing"`
		// ContextBodyUnexpected are indexed units with a context_body while their
		// Postgres context is blank.
		ContextBodyUnexpected int64 `json:"context_body_unexpected"`
	} `json:"difference"`
}

// UnavailableError says the parity evidence could not be read. The gate reports
// it as unavailable, never as passing. Reason is content-free.
type UnavailableError struct{ Reason string }

func (e *UnavailableError) Error() string { return "keyword index parity unavailable: " + e.Reason }

func unavailable(format string, args ...any) error {
	return &UnavailableError{Reason: fmt.Sprintf(format, args...)}
}

// ParitySource reads the keyword index's parity report for the organization of
// the verified caller.
type ParitySource interface {
	Fetch(ctx context.Context) (*ParityReport, error)
}

// HTTPParity reads the parity endpoint over HTTP.
//
// The organization is never sent: the endpoint measures the organization of the
// bearer it is called with. The bearer is the caller's own, already verified for
// this service's audience (authctx.AuthorizationHeader), forwarded in memory to
// the one URL the operator configured; it is never stored or logged. The endpoint
// verifies it again and requires data:quality:admin.
type HTTPParity struct {
	url     string
	client  *http.Client
	timeout time.Duration
}

// maxParityBody bounds what is read from the endpoint: the report is a few
// hundred bytes.
const maxParityBody = 64 << 10

// NewHTTPParity returns a source for the endpoint at url (an absolute http(s)
// URL, already checked by config). A nil client gets a plain one; the timeout
// bounds one fetch.
func NewHTTPParity(url string, client *http.Client, timeout time.Duration) *HTTPParity {
	if client == nil {
		// A redirect is never followed: it would carry the caller's bearer to
		// wherever the endpoint points, and the contract has no redirects.
		client = &http.Client{CheckRedirect: func(*http.Request, []*http.Request) error {
			return http.ErrUseLastResponse
		}}
	}
	if timeout <= 0 {
		timeout = 5 * time.Second
	}
	return &HTTPParity{url: url, client: client, timeout: timeout}
}

// Fetch calls the endpoint. Every failure to obtain a well-formed report is an
// *UnavailableError: not configured, no credential to forward, the endpoint is
// not there yet, it refused the credential, it errored, it timed out, or it
// answered with something that is not the contract.
func (p *HTTPParity) Fetch(ctx context.Context) (*ParityReport, error) {
	bearer, ok := authctx.AuthorizationHeader(ctx)
	if !ok {
		return nil, unavailable("there is no caller credential to forward to the endpoint")
	}
	ctx, cancel := context.WithTimeout(ctx, p.timeout)
	defer cancel()
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, p.url, nil)
	if err != nil {
		return nil, unavailable("the configured endpoint URL is not usable")
	}
	req.Header.Set("Authorization", bearer)
	req.Header.Set("Accept", "application/json")

	resp, err := p.client.Do(req)
	if err != nil {
		return nil, unavailable("the endpoint could not be reached (%s)", transportReason(err))
	}
	defer resp.Body.Close()
	switch {
	case resp.StatusCode == http.StatusNotFound || resp.StatusCode == http.StatusNotImplemented:
		return nil, unavailable("the endpoint does not exist on the adapter that answered (it ships with CR-T9a)")
	case resp.StatusCode == http.StatusUnauthorized || resp.StatusCode == http.StatusForbidden:
		return nil, unavailable("the endpoint refused the caller's credential (HTTP %d)", resp.StatusCode)
	case resp.StatusCode != http.StatusOK:
		return nil, unavailable("the endpoint answered HTTP %d", resp.StatusCode)
	}
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxParityBody+1))
	if err != nil {
		return nil, unavailable("the response could not be read")
	}
	if len(body) > maxParityBody {
		return nil, unavailable("the response is larger than %d bytes", maxParityBody)
	}
	var report ParityReport
	if err := json.Unmarshal(body, &report); err != nil {
		return nil, unavailable("the response is not the %s JSON", ParityContract)
	}
	return &report, nil
}

func transportReason(err error) string {
	switch {
	case errors.Is(err, context.DeadlineExceeded):
		return "timed out"
	case errors.Is(err, context.Canceled):
		return "cancelled"
	default:
		return "connection failed"
	}
}

// PostgresSide is what this service itself counts in Postgres, with the same
// predicate the adapter indexes by. It is read independently of the adapter on
// purpose: if the two disagree about how many units are eligible, the gate does
// not trust either (a drifted predicate would otherwise make parity meaningless).
type PostgresSide struct {
	EligibleUnits       int64
	EligibleWithContext int64
}

const postgresSideSQL = `
	SELECT count(*),
	       count(*) FILTER (WHERE ku.chunk_context IS NOT NULL AND btrim(ku.chunk_context) <> '')
	  FROM knowledge_units ku
	  JOIN documents d ON d.document_id = ku.document_id
	 WHERE ku.org_id = $1
	   AND ku.embedding_status = 'done'
	   AND d.deleted_at IS NULL
	   AND lower(btrim(d.zdr_classification)) IN ('internal', 'public', 'sensitive')`

func readPostgresSide(ctx context.Context, q queryer, orgID string) (PostgresSide, error) {
	var side PostgresSide
	if err := q.QueryRow(ctx, postgresSideSQL, orgID).Scan(&side.EligibleUnits, &side.EligibleWithContext); err != nil {
		return PostgresSide{}, err
	}
	return side, nil
}

func unavailableParity(reason string) model.GateResult {
	return model.GateResult{
		Gate:    GateSparseIndexInSync,
		Passed:  false,
		Status:  StatusUnavailable,
		Message: "keyword index parity cannot be read: " + reason + " (never reported as passing)",
	}
}

func failedParity(value, format string, args ...any) model.GateResult {
	return model.GateResult{
		Gate:    GateSparseIndexInSync,
		Passed:  false,
		Status:  StatusFailed,
		Message: fmt.Sprintf(format, args...),
		Value:   value,
	}
}

// EvaluateSparseParity is the rule of the sparse_index_in_sync gate: the keyword
// index holds exactly the units Postgres says it should, each with its context
// where Postgres has one.
//
// pg is this service's own count (pgErr when it could not be read), report the
// adapter's measurement (fetchErr when it could not be obtained). The gate passes
// only when both were obtained, the report is the contract for this organization
// and is arithmetically consistent, the adapter and this service agree on the
// expected counts, and the four differences are zero. Anything it cannot read is
// "unavailable", which is not a pass.
func EvaluateSparseParity(orgID string, pg PostgresSide, pgErr error, report *ParityReport, fetchErr error) model.GateResult {
	if fetchErr != nil {
		var unavailableErr *UnavailableError
		if errors.As(fetchErr, &unavailableErr) {
			return unavailableParity(unavailableErr.Reason)
		}
		return unavailableParity("the endpoint could not be read")
	}
	if pgErr != nil {
		return unavailableParity("the database side could not be counted")
	}
	if report == nil {
		return unavailableParity("the endpoint returned no report")
	}
	if report.Contract != ParityContract {
		return unavailableParity(fmt.Sprintf("the endpoint speaks contract %q, this service reads %q", report.Contract, ParityContract))
	}
	if report.OrgID != orgID {
		// Never echo the other organization's id or numbers.
		return failedParity("", "the parity report is for a different organization than the caller's: refusing to judge it")
	}

	d := report.Difference
	if d.MissingUnits < 0 || d.UnexpectedUnits < 0 || d.ContextBodyMissing < 0 || d.ContextBodyUnexpected < 0 ||
		report.Postgres.EligibleUnits < 0 || report.Quickwit.DistinctUnits < 0 {
		return failedParity("", "the parity report has negative counts: the adapter's measurement is not trustworthy")
	}
	// The report must add up: expected, minus what is missing, plus what is stale,
	// is what the index holds.
	if report.Postgres.EligibleUnits-d.MissingUnits+d.UnexpectedUnits != report.Quickwit.DistinctUnits {
		return failedParity("", "the parity report does not add up (eligible %d - missing %d + unexpected %d is not the %d units indexed)",
			report.Postgres.EligibleUnits, d.MissingUnits, d.UnexpectedUnits, report.Quickwit.DistinctUnits)
	}
	if report.Postgres.EligibleUnits != pg.EligibleUnits || report.Postgres.EligibleWithContext != pg.EligibleWithContext {
		return failedParity("",
			"the adapter and the database disagree about what is eligible (adapter: %d units, %d with a context; database: %d units, %d with a context). "+
				"Re-run when ingestion is quiet; a difference that persists means the two services use different eligibility rules",
			report.Postgres.EligibleUnits, report.Postgres.EligibleWithContext, pg.EligibleUnits, pg.EligibleWithContext)
	}

	value := fmt.Sprintf("%d/%d", report.Quickwit.DistinctUnits, report.Postgres.EligibleUnits)
	if d.MissingUnits > 0 || d.UnexpectedUnits > 0 || d.ContextBodyMissing > 0 || d.ContextBodyUnexpected > 0 {
		return failedParity(value,
			"the keyword index is not in sync with Postgres: %d of %d eligible units indexed (%d missing, %d stale), %d indexed without the context Postgres has, %d with a context Postgres does not have",
			report.Quickwit.DistinctUnits, report.Postgres.EligibleUnits, d.MissingUnits, d.UnexpectedUnits, d.ContextBodyMissing, d.ContextBodyUnexpected)
	}
	message := fmt.Sprintf("the keyword index holds all %d eligible units, %d with their context", report.Quickwit.DistinctUnits, report.Quickwit.WithContextBody)
	if report.Quickwit.Documents > report.Quickwit.DistinctUnits {
		message += fmt.Sprintf(" (%d duplicate documents await a merge; retrieval de-duplicates by knowledge id)", report.Quickwit.Documents-report.Quickwit.DistinctUnits)
	}
	return model.GateResult{Gate: GateSparseIndexInSync, Passed: true, Status: StatusPassed, Message: message, Value: value}
}
