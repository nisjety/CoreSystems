package activities

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/stretchr/testify/require"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/errs"
)

// changeServer plays both upstreams of CheckChange: the runtime's run_page (a
// fixed fingerprint) and the edge's change record route (a canned status and
// body). recordCalls counts the record attempts so a test can prove no retry
// happened inside the activity.
func changeServer(t *testing.T, recordStatus int, recordBody string, recordCalls *atomic.Int32) *httptest.Server {
	t.Helper()
	mux := http.NewServeMux()
	mux.HandleFunc("/v1/internal/run_page", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"status":200,"fingerprint":"blake3:fresh","links":[]}`))
	})
	mux.HandleFunc("/v1/internal/change/record", func(w http.ResponseWriter, r *http.Request) {
		recordCalls.Add(1)
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(recordStatus)
		_, _ = w.Write([]byte(recordBody))
	})
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)
	return srv
}

func newChangeActivities(srv *httptest.Server) *Activities {
	return New(Config{RuntimeBaseURL: srv.URL, EdgeBaseURL: srv.URL})
}

// The edge's enforced refusal (crates/quarry-edge/src/zdr_refusal.rs): a 403
// error envelope with details.zdr_refusal.
const zdrRefusalBody = `{"data":null,"meta":{"request_id":"req_1"},"error":{"code":"FORBIDDEN",` +
	`"message":"ZDR active: durable resources are not available under zero data retention (the resource would be retained)",` +
	`"details":{"zdr_refusal":{"code":"zdr_durable_mutation_forbidden","action":"record"}}}}`

func TestCheckChange_ZDRRefusalIsATerminalOutcomeNotAnError(t *testing.T) {
	var calls atomic.Int32
	srv := changeServer(t, http.StatusForbidden, zdrRefusalBody, &calls)

	res, err := newChangeActivities(srv).CheckChange(context.Background(), CheckChangeInput{
		RunID: "run_1", OrgID: "org_zdr", URL: "https://example.com/p",
	})

	require.NoError(t, err, "a ZDR refusal must not surface as an activity error (it would be CategoryAuth)")
	require.True(t, res.ZDRRefused)
	require.Equal(t, StatusZDRRefused, res.Status)
	require.False(t, res.Changed)
	require.Empty(t, res.BaselineID)
	require.Empty(t, res.DiffID)
	require.Equal(t, int32(1), calls.Load(), "no retry of the record call")
}

// A 403 that is NOT the ZDR refusal (a wrong internal token, a bot wall, a
// proxy page) keeps its non-retryable auth classification: only the catalogue
// code turns a 403 into a ZDR outcome.
func TestCheckChange_OtherForbiddenStaysAnAuthFailure(t *testing.T) {
	for name, body := range map[string]string{
		"plain text":          `forbidden`,
		"error without zdr":   `{"error":{"code":"FORBIDDEN","message":"bad internal token"}}`,
		"another refusal":     `{"error":{"code":"FORBIDDEN","message":"x","details":{"zdr_refusal":{"code":"something_else"}}}}`,
		"a bot wall reason":   `{"error":{"code":"FORBIDDEN","message":"x","details":{"reason":"bot_wall"}}}`,
		"code in the message": `{"error":{"code":"FORBIDDEN","message":"zdr_durable_mutation_forbidden"}}`,
	} {
		t.Run(name, func(t *testing.T) {
			var calls atomic.Int32
			srv := changeServer(t, http.StatusForbidden, body, &calls)

			res, err := newChangeActivities(srv).CheckChange(context.Background(), CheckChangeInput{
				RunID: "run_1", OrgID: "org_a", URL: "https://example.com/p",
			})

			require.Error(t, err)
			require.False(t, res.ZDRRefused)
			require.True(t, errs.IsNonRetryable(err))
			require.Contains(t, err.Error(), errs.CategoryAuth.String())
		})
	}
}

// The refusal code on any status other than 403 is not a ZDR outcome either.
func TestCheckChange_ZDRCodeOnAnotherStatusIsNotAZDROutcome(t *testing.T) {
	var calls atomic.Int32
	srv := changeServer(t, http.StatusInternalServerError, zdrRefusalBody, &calls)

	res, err := newChangeActivities(srv).CheckChange(context.Background(), CheckChangeInput{
		RunID: "run_1", OrgID: "org_a", URL: "https://example.com/p",
	})

	require.Error(t, err)
	require.False(t, res.ZDRRefused)
	require.False(t, errs.IsNonRetryable(err), "a 5xx stays retryable")
}

// The unchanged path is untouched by the new branch.
func TestCheckChange_SuccessStillDecodesTheRecord(t *testing.T) {
	var calls atomic.Int32
	srv := changeServer(t, http.StatusOK,
		`{"status":"changed","changed":true,"fingerprint":"blake3:fresh","baseline_id":"bln_2","diff_id":"diff_1"}`, &calls)

	res, err := newChangeActivities(srv).CheckChange(context.Background(), CheckChangeInput{
		RunID: "run_1", OrgID: "org_a", URL: "https://example.com/p",
	})

	require.NoError(t, err)
	require.False(t, res.ZDRRefused)
	require.True(t, res.Changed)
	require.Equal(t, "diff_1", res.DiffID)
}

func TestIsZDRRefusal(t *testing.T) {
	require.True(t, isZDRRefusal(http.StatusForbidden, []byte(zdrRefusalBody)))
	require.False(t, isZDRRefusal(http.StatusUnauthorized, []byte(zdrRefusalBody)))
	require.False(t, isZDRRefusal(http.StatusForbidden, []byte(strings.Repeat("x", 4096))))
	require.False(t, isZDRRefusal(http.StatusForbidden, nil))
}
