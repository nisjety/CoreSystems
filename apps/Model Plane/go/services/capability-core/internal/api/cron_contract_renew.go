package api

import (
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	"github.com/triodelab/model-plane/services/capability-core/internal/cron"
)

// Renewing a lapsed or lapsing schedule approval is an in-place PATCH of the existing schedule
// (O-10, D-F05-1). The schedule keeps its id, fire history and counters; only the stored
// `approval_contract` text and the digest of the template that carries it change. Authority stays
// with Control: the PATCH carries a decision Control signed for exactly this schedule, this
// contract text and the template that results, under a schema no create decision can satisfy.

// cronRenewalAuditAction is the registry audit action of a renewal. The row carries the contract
// digest and expiry only, never the contract text, the goal or any tool arguments.
const cronRenewalAuditAction = "schedule_contract_renewed"

// renewalEnabled is true when stored-consent contracts and their guards are both on. Off, a PATCH
// that carries the renewal fields is refused as not_supported before anything is read or written,
// and a PATCH without them behaves exactly as before.
func (h *CronHandler) renewalEnabled() bool {
	return h.scheduleContract && h.contractGuards
}

// cronRenewalBody is what a renewing PATCH adds to enabled and schedule_expr. The decision token is
// the same bearer-style Control decision a create carries; it is never stored or returned.
type cronRenewalBody struct {
	Enabled          *bool
	ScheduleExpr     *string
	ApprovalContract *string
	Decision         string
	IdempotencyKey   string
}

// renewalAuditID is deterministic in the decision's idempotency key, so a decision can be applied
// once: a second PATCH with the same key collides on the audit table's primary key and the whole
// statement, UPDATE included, is refused.
func renewalAuditID(orgID, scheduleID, idempotencyKey string) string {
	sum := sha256.Sum256([]byte(orgID + "\x00" + scheduleID + "\x00" + idempotencyKey))
	return fmt.Sprintf("cron-renew-%x", sum[:16])
}

// cronRenewalUpdateSQL replaces the template and its digest in one statement and appends the audit
// row in the same statement. The update is guarded on the template it was computed from, so a
// concurrent change makes it match no row. Parameters: $1 new template, $2 new template digest,
// $3 enabled, $4 schedule_expr, $5 next_fire_at, $6 now, $7 schedule id, $8 org, $9 actor,
// $10 old template, $11 audit id, $12 audit diff.
const cronRenewalUpdateSQL = `
WITH upd AS (
    UPDATE cron_schedules
    SET task_template=$1::jsonb, template_digest=$2, enabled=COALESCE($3, enabled),
        schedule_expr=COALESCE($4, schedule_expr), next_fire_at=$5, updated_at=$6
    WHERE id=$7 AND org_id=$8 AND creator_subject_id=$9 AND deleted_at IS NULL AND task_template=$10::jsonb
    RETURNING id
), aud AS (
    INSERT INTO registry_audit_log (id, entity_kind, entity_id, action, actor, org_id, diff_json)
    SELECT $11::text, 'cron_schedule', $7::text, '` + cronRenewalAuditAction + `', $9::text, $8::text, $12::jsonb FROM upd
    RETURNING 1
)
SELECT count(*) FROM upd
`

// renewScheduleContract applies one renewal. It answers the request itself.
func (h *CronHandler) renewScheduleContract(w http.ResponseWriter, r *http.Request, id string, body cronRenewalBody) {
	if body.ApprovalContract == nil || strings.TrimSpace(*body.ApprovalContract) == "" ||
		strings.TrimSpace(body.Decision) == "" || strings.TrimSpace(body.IdempotencyKey) == "" {
		jsonErr(w, "a renewal needs approval_contract, the schedule renewal decision and idempotency_key", http.StatusBadRequest)
		return
	}
	if body.ScheduleExpr != nil && strings.TrimSpace(*body.ScheduleExpr) == "" {
		jsonErr(w, "schedule expression must not be empty", http.StatusBadRequest)
		return
	}
	if h.verifier == nil {
		jsonErr(w, "Space schedule authorization is unavailable", http.StatusServiceUnavailable)
		return
	}
	orgID := verifiedOrganizationID(r)
	actor := verifiedActorID(r)
	now := time.Now().UTC()

	var (
		oldTemplate []byte
		creator     string
		spaceRef    string
		expr        string
		timezone    string
	)
	err := h.pool.QueryRow(r.Context(), `
		SELECT task_template, creator_subject_id, space_ref, schedule_expr, timezone
		FROM cron_schedules WHERE id=$1 AND org_id=$2 AND deleted_at IS NULL
	`, id, orgID).Scan(&oldTemplate, &creator, &spaceRef, &expr, &timezone)
	if errors.Is(err, pgx.ErrNoRows) {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	// Only the person who gave the approval can renew it. An org admin may pause or delete, and
	// still cannot put their own approval behind another person's schedule.
	if actor == "" || creator == "" || actor != creator {
		jsonErr(w, "only the schedule's creator can renew its approval", http.StatusForbidden)
		return
	}

	var template map[string]any
	if err := json.Unmarshal(oldTemplate, &template); err != nil || template == nil {
		jsonErr(w, "the schedule's template cannot be read", http.StatusConflict)
		return
	}
	if _, has := template["approval_contract"]; !has {
		jsonErr(w, "the schedule has no approval to renew", http.StatusConflict)
		return
	}

	// Shape, window, registry and digest of the new contract, exactly as a create checks them.
	text := *body.ApprovalContract
	intent, err := h.createIntentFor(r.Context(), orgID, map[string]any{"approval_contract": text}, cron.CreateIntent{})
	if err != nil {
		jsonErr(w, "schedule contract refused: "+err.Error(), http.StatusBadRequest)
		return
	}
	template["approval_contract"] = text
	newTemplate, err := json.Marshal(template)
	if err != nil {
		jsonErr(w, "the schedule's template cannot be encoded", http.StatusInternalServerError)
		return
	}
	newDigest := fmt.Sprintf("sha256:%x", sha256.Sum256(newTemplate))

	intent.OrgID = orgID
	intent.SpaceRef = spaceRef
	intent.SubjectID = creator
	intent.ScheduleID = id
	intent.TemplateDigest = newDigest
	intent.IdempotencyKey = body.IdempotencyKey
	intent.RenewalOf = id
	if _, err := h.verifier.VerifyScheduleCreate(body.Decision, intent, now); err != nil {
		if errors.Is(err, cron.ErrDecisionPayloadMismatch) {
			// The decision is genuine but binds another contract or template than the one sent.
			jsonErr(w, "the renewal decision does not bind this contract", http.StatusBadRequest)
			return
		}
		jsonErr(w, "Space schedule renewal is not authorized", http.StatusForbidden)
		return
	}

	if body.ScheduleExpr != nil {
		expr = *body.ScheduleExpr
	}
	next, err := cron.NextFrom(expr, timezone, now)
	if err != nil {
		jsonErr(w, "invalid cron expression: "+err.Error(), http.StatusBadRequest)
		return
	}
	diff, _ := json.Marshal(map[string]any{
		"op": "renew", "contract_digest": intent.ContractDigest, "contract_expires_at": intent.ContractExpiresAt,
	})
	var updated int
	err = h.pool.QueryRow(r.Context(), cronRenewalUpdateSQL,
		newTemplate, newDigest, body.Enabled, body.ScheduleExpr, next, now, id, orgID, creator, oldTemplate,
		renewalAuditID(orgID, id, body.IdempotencyKey), string(diff),
	).Scan(&updated)
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) && pgErr.Code == "23505" {
		jsonErr(w, "this renewal decision was already used", http.StatusConflict)
		return
	}
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	if updated != 1 {
		jsonErr(w, "the schedule changed while it was being renewed; try again", http.StatusConflict)
		return
	}
	writeJSON(w, map[string]any{"id": id, "updated_at": now, "next_fire_at": next, "renewed": true})
}
