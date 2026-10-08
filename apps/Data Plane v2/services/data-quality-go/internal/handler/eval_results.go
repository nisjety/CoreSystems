package handler

import (
	"errors"
	"io"
	"net/http"
	"time"

	"github.com/google/uuid"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/authctx"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/eval"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/model"
)

// WithResultRecorder enables POST /v1/evals/results. Without a recorder the route answers 503: the
// service never pretends to have recorded a run.
func (h *QualityHandler) WithResultRecorder(recorder eval.ResultRecorder) *QualityHandler {
	h.recorder = recorder
	return h
}

// RecordEvalResult stores the summary of a retrieval-quality run that was measured outside this service
// (scripts/eval-gate.py), in quality_eval_runs, so that a regression is a query.
//
// Who may call: an OPERATOR (a verified service principal holding data:quality:admin), for the
// organization of its own token. An organization's own admin holds the scope too and must not be able to
// write history that a promotion decision reads, so the user principals are refused (the route's
// requireOperator, and again here). The organization is never in the body: a body that names one is refused
// as an unknown field.
//
// The Idempotency-Key header is required. A replay with the same key and content is a 200 with the stored
// run; the same key with other content is a 409, because history is immutable.
func (h *QualityHandler) RecordEvalResult(w http.ResponseWriter, r *http.Request) {
	if h.recorder == nil {
		writeCodedError(w, http.StatusServiceUnavailable, "results_store_unavailable", "the evaluation results store is not configured")
		return
	}
	claims, ok := authctx.FromContext(r.Context())
	if !ok {
		writeCodedError(w, http.StatusUnauthorized, "unauthorized", "authentication required")
		return
	}
	// Defence in depth: the route's middleware already refused these.
	if !claims.IsService() {
		writeCodedError(w, http.StatusForbidden, "operator_only", "evaluation results can only be recorded by an operator service principal")
		return
	}
	key, err := requestIdempotencyKey(r)
	if err != nil {
		writeCodedError(w, http.StatusBadRequest, "invalid_idempotency_key", err.Error())
		return
	}

	raw, err := io.ReadAll(http.MaxBytesReader(w, r.Body, eval.MaxRecordBytes))
	if err != nil {
		var tooLarge *http.MaxBytesError
		if errors.As(err, &tooLarge) {
			writeCodedError(w, http.StatusRequestEntityTooLarge, "record_too_large", "a record is a summary of at most 64 KiB")
			return
		}
		writeCodedError(w, http.StatusBadRequest, "invalid_request", "the body could not be read")
		return
	}
	record, err := eval.ValidateRecord(raw)
	if err != nil {
		var invalid *eval.InvalidRecordError
		if errors.As(err, &invalid) {
			writeJSON(w, http.StatusBadRequest, map[string]any{
				"error":    "the record is not valid",
				"code":     "invalid_record",
				"problems": invalid.Problems,
			})
			return
		}
		writeCodedError(w, http.StatusBadRequest, "invalid_record", "the record is not valid")
		return
	}

	now := time.Now().UTC()
	run := model.EvalRun{
		EvalID:         uuid.NewString(),
		OrgID:          claims.OrgID,
		Strategy:       record.Strategy,
		Status:         model.EvalCompleted,
		Corpus:         record.Corpus,
		Scorecard:      record.Scorecard,
		IdempotencyKey: key,
		CreatedAt:      now,
		UpdatedAt:      now,
	}
	saved, created, err := h.recorder.Record(r.Context(), run)
	if errors.Is(err, eval.ErrIdempotencyConflict) {
		writeCodedError(w, http.StatusConflict, "idempotency_conflict", "the idempotency key is bound to a different record: history is immutable")
		return
	}
	if err != nil {
		log.Error().Err(err).Str("org_id", claims.OrgID).Str("strategy", record.Strategy).Msg("evaluation result: store failure")
		writeCodedError(w, http.StatusInternalServerError, "record_failed", "the evaluation result could not be recorded")
		return
	}
	status := http.StatusOK
	if created {
		status = http.StatusCreated
		log.Info().
			Str("org_id", claims.OrgID).
			Str("principal", claims.PrincipalID()).
			Str("strategy", record.Strategy).
			Str("cell", record.Harness.Cell).
			Str("run_id", record.Harness.RunID).
			Str("verdict", record.Harness.Gate.Verdict).
			Msg("evaluation result recorded")
	}
	writeJSON(w, status, saved)
}
