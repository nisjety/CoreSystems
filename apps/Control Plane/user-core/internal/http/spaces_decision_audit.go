package http

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"log"
	"net/http"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS05-S5: Space decision audit events.
//
// One group middleware observes the issuer routes (spaces.DecisionAuditRouteFor)
// and, after the handler has answered, writes a content-free row to
// user_audit_outbox:
//   - a 403 the handler itself produced -> space_decision_denied (a refusal from
//     a scope guard aborts the chain and is not audited here: it is not a Space
//     authority decision);
//   - a 409 or 503 the handler produced -> space_decision_denied too (B-REC-03),
//     with outcome "denied" for a conflict and "error" for an availability
//     refusal;
//   - a 200 carrying data.decision on an effect-bound route -> space_decision_issued;
//   - a 200 of the batch thread-read-decisions route -> one space_decision_denied
//     row per denied result row (B-REC-03).
//
// Every row carries actor_role (delegated_user, service or user).
//
// A refusal is never changed: its row is best effort, an outbox failure is
// logged and a refusal stays a refusal. An effect-bound issue fails closed
// (B-REC-03): its response is held until the issued row is in the outbox, and
// when the row cannot be built or written the caller gets a typed 503
// (space_decision_audit_unavailable) instead of the decision and token, so no
// effect can be authorized by a decision the org audit trail does not hold.
// A handler whose answer does not carry the decision object (sandbox-capability
// answers data.decision as the token string) hands the decision over the gin
// context (handOffIssuedDecisionForAudit); an effect-bound 200 with no
// attributable decision is withheld behind the same 503.
// The request body is read for four identifiers (org, subject, Space, action)
// and restored. The token, payload digest, nonce and recipient references are
// never read into the event.

const (
	decisionAuditMaxRequestBody  = 1 << 20
	decisionAuditMaxResponseBody = 64 << 10
	// A full batch (60 rows, each with a token) is larger than one decision.
	decisionAuditMaxBatchResponseBody = 1 << 20
	decisionAuditWriteTimeout         = 3 * time.Second

	// decisionAuditUnavailableCode is the typed refusal of an effect-bound issue
	// whose audit row could not be recorded.
	decisionAuditUnavailableCode = "space_decision_audit_unavailable"
)

// decisionAuditSink is the wired producer: the flags plus the outbox.
type decisionAuditSink struct {
	cfg    spaces.DecisionAuditConfig
	outbox spaces.DecisionAuditOutbox
}

// SetSpaceDecisionAudit wires the Space decision audit producer. An unwired
// outbox or a disabled flag makes the middleware a pass-through.
func (s *Server) SetSpaceDecisionAudit(cfg spaces.DecisionAuditConfig, outbox spaces.DecisionAuditOutbox) {
	s.spaceDecisionAudit = decisionAuditSink{cfg: cfg, outbox: outbox}
}

// decisionAuditWriter copies a bounded prefix of the response body.
type decisionAuditWriter struct {
	gin.ResponseWriter
	buf       bytes.Buffer
	limit     int
	truncated bool
}

func (w *decisionAuditWriter) capture(p []byte) {
	if room := w.limit - w.buf.Len(); room > 0 {
		if len(p) > room {
			w.buf.Write(p[:room])
			w.truncated = true
		} else {
			w.buf.Write(p)
		}
	} else if len(p) > 0 {
		w.truncated = true
	}
}

func (w *decisionAuditWriter) Write(p []byte) (int, error) {
	w.capture(p)
	return w.ResponseWriter.Write(p)
}

func (w *decisionAuditWriter) WriteString(s string) (int, error) {
	w.capture([]byte(s))
	return w.ResponseWriter.WriteString(s)
}

// decisionAuditHoldWriter holds the whole answer of an effect-bound issuer
// route: nothing reaches the client until the middleware has decided whether
// to release it or to replace it with the fail-closed refusal. Headers set by
// the handler go straight to the real header map.
type decisionAuditHoldWriter struct {
	gin.ResponseWriter
	buf     bytes.Buffer
	status  int
	written bool
}

func (w *decisionAuditHoldWriter) WriteHeader(code int) {
	if code > 0 && !w.written {
		w.status = code
	}
}

func (w *decisionAuditHoldWriter) WriteHeaderNow() { w.written = true }

func (w *decisionAuditHoldWriter) Write(p []byte) (int, error) {
	w.written = true
	return w.buf.Write(p)
}

func (w *decisionAuditHoldWriter) WriteString(s string) (int, error) {
	w.written = true
	return w.buf.WriteString(s)
}

func (w *decisionAuditHoldWriter) Status() int {
	if w.status == 0 {
		return http.StatusOK
	}
	return w.status
}

func (w *decisionAuditHoldWriter) Size() int {
	if !w.written {
		return -1
	}
	return w.buf.Len()
}

func (w *decisionAuditHoldWriter) Written() bool { return w.written }

// Flush is held too: a flush would commit the status before the audit row.
func (w *decisionAuditHoldWriter) Flush() {}

// release sends the held answer unchanged.
func (w *decisionAuditHoldWriter) release() {
	if !w.written && w.status == 0 {
		return
	}
	w.ResponseWriter.WriteHeader(w.Status())
	if w.buf.Len() > 0 {
		_, _ = w.ResponseWriter.Write(w.buf.Bytes())
	} else {
		w.ResponseWriter.WriteHeaderNow()
	}
}

// refuse drops the held answer (decision and token included) and sends the
// typed fail-closed refusal instead.
func (w *decisionAuditHoldWriter) refuse() {
	body, _ := json.Marshal(gin.H{"error": gin.H{
		"code":    decisionAuditUnavailableCode,
		"message": "Space decision audit unavailable; the decision was not released",
		"details": gin.H{},
	}})
	header := w.ResponseWriter.Header()
	header.Del("Content-Length")
	header.Set("Content-Type", "application/json; charset=utf-8")
	w.ResponseWriter.WriteHeader(http.StatusServiceUnavailable)
	_, _ = w.ResponseWriter.Write(body)
}

// decisionAuditRequestIDs are the only request fields the middleware reads.
// Intent-style bodies nest the same keys under "intent".
type decisionAuditRequestIDs struct {
	OrgID     string `json:"org_id"`
	SubjectID string `json:"subject_id"`
	SpaceRef  string `json:"space_ref"`
	ActionID  string `json:"action_id"`
	Intent    *struct {
		OrgID     string `json:"org_id"`
		SubjectID string `json:"subject_id"`
		SpaceRef  string `json:"space_ref"`
		ActionID  string `json:"action_id"`
	} `json:"intent"`
}

func (r decisionAuditRequestIDs) flattened() decisionAuditRequestIDs {
	if r.Intent != nil {
		if r.OrgID == "" {
			r.OrgID = r.Intent.OrgID
		}
		if r.SubjectID == "" {
			r.SubjectID = r.Intent.SubjectID
		}
		if r.SpaceRef == "" {
			r.SpaceRef = r.Intent.SpaceRef
		}
		if r.ActionID == "" {
			r.ActionID = r.Intent.ActionID
		}
	}
	return r
}

// decisionAuditResponse is the allowlisted view of an answer. The token field
// is deliberately absent.
type decisionAuditResponse struct {
	// Typed refusal body: {"code":...} or the envelope {"error":{"code":...}}.
	Code         string          `json:"code"`
	ResolvableBy string          `json:"resolvable_by"`
	Error        json.RawMessage `json:"error"`
	// Issued body: {"data":{"decision":{...}}}.
	Data *struct {
		Decision *decisionAuditIssuedView `json:"decision"`
	} `json:"data"`
}

// decisionAuditIssuedView is the allowlisted view of an issued decision.
type decisionAuditIssuedView struct {
	DecisionRef       string    `json:"decision_ref"`
	OrgID             string    `json:"org_id"`
	SpaceRef          string    `json:"space_ref"`
	SubjectID         string    `json:"subject_id"`
	ServiceAudience   string    `json:"service_audience"`
	ActionID          string    `json:"action_id"`
	AuthorityRevision int64     `json:"authority_revision"`
	ExpiresAt         time.Time `json:"expires_at"`
	ZeroDataRetention bool      `json:"zero_data_retention"`
}

// spaceDecisionAuditIssuedKey is the gin context key a handler whose answer
// does not carry the decision object (sandbox-capability answers
// data.decision as the token string) uses to hand the issued decision to the
// audit middleware.
const spaceDecisionAuditIssuedKey = "space_decision_audit_issued"

// handOffIssuedDecisionForAudit hands the issued decision to the audit
// middleware without changing the answer's shape.
func handOffIssuedDecisionForAudit(c *gin.Context, decision any) {
	c.Set(spaceDecisionAuditIssuedKey, decision)
}

// issuedDecisionForAudit finds the decision an effect-bound 200 issued: the
// handed-off one first, else data.decision in the held body. ok=false means a
// 200 the middleware cannot attribute to a decision.
func issuedDecisionForAudit(c *gin.Context, body []byte) (*decisionAuditIssuedView, bool) {
	if handed, exists := c.Get(spaceDecisionAuditIssuedKey); exists && handed != nil {
		raw, err := json.Marshal(handed)
		if err != nil {
			return nil, false
		}
		var view decisionAuditIssuedView
		if json.Unmarshal(raw, &view) != nil {
			return nil, false
		}
		return &view, true
	}
	var answer decisionAuditResponse
	if json.Unmarshal(body, &answer) != nil || answer.Data == nil || answer.Data.Decision == nil {
		return nil, false
	}
	return answer.Data.Decision, true
}

// refusalCode is the typed code of a refusal in either body shape.
func (r decisionAuditResponse) refusalCode() (code, resolvableBy string) {
	code, resolvableBy = r.Code, r.ResolvableBy
	if code == "" && len(r.Error) > 0 && r.Error[0] == '{' {
		var envelope struct {
			Code    string `json:"code"`
			Details struct {
				ResolvableBy string `json:"resolvable_by"`
			} `json:"details"`
		}
		if json.Unmarshal(r.Error, &envelope) == nil {
			code = envelope.Code
			if resolvableBy == "" {
				resolvableBy = envelope.Details.ResolvableBy
			}
		}
	}
	return code, resolvableBy
}

// decisionAuditBatchResponse is the allowlisted view of a batch answer: the
// per-row decision and token are deliberately absent.
type decisionAuditBatchResponse struct {
	Data *struct {
		Results []struct {
			SpaceRef string `json:"space_ref"`
			Status   string `json:"status"`
			Reason   string `json:"reason"`
		} `json:"results"`
	} `json:"data"`
}

// decisionAuditActorRole is who the issuer call speaks for. The gateway's
// verified delegation speaks for a user; any other service principal for
// itself; a bearer caller is a user (unreachable on the internal issuer group
// today, whose service-principal guard rejects bearer callers first; kept so the
// label stays right if a bearer route is ever audited). It is an audit label,
// never authority.
func decisionAuditActorRole(c *gin.Context) string {
	switch c.GetString("auth_method") {
	case "service_principal":
		if c.GetBool("delegation_verified") {
			return spaces.DecisionAuditActorDelegatedUser
		}
		return spaces.DecisionAuditActorService
	case "bearer":
		return spaces.DecisionAuditActorUser
	}
	return ""
}

func isAuditedRefusal(status int) bool {
	return status == http.StatusForbidden || status == http.StatusConflict || status == http.StatusServiceUnavailable
}

func (s *Server) spaceDecisionAuditMiddleware() gin.HandlerFunc {
	return func(c *gin.Context) {
		sink := s.spaceDecisionAudit
		if !sink.cfg.Enabled || sink.outbox == nil || c.Request.Method != http.MethodPost {
			c.Next()
			return
		}
		route, row, ok := spaces.DecisionAuditRouteFor(c.Request.URL.Path)
		auditIssued := sink.cfg.Issued && row.EffectBound
		if !ok || (!sink.cfg.Denied && !auditIssued) {
			c.Next()
			return
		}

		var ids decisionAuditRequestIDs
		if c.Request.Body != nil {
			original := c.Request.Body
			head, _ := io.ReadAll(io.LimitReader(original, decisionAuditMaxRequestBody))
			c.Request.Body = struct {
				io.Reader
				io.Closer
			}{io.MultiReader(bytes.NewReader(head), original), original}
			_ = json.Unmarshal(head, &ids)
			ids = ids.flattened()
		}
		actorRole := decisionAuditActorRole(c)

		if auditIssued {
			holdAndAuditEffectBound(c, sink, route, row, ids, actorRole)
			return
		}

		limit := decisionAuditMaxResponseBody
		if row.Batch {
			limit = decisionAuditMaxBatchResponseBody
		}
		writer := &decisionAuditWriter{ResponseWriter: c.Writer, limit: limit}
		c.Writer = writer
		c.Next()
		if c.IsAborted() {
			return
		}
		if writer.truncated {
			if isAuditedRefusal(c.Writer.Status()) || row.Batch {
				log.Printf("user-core space decision audit: %s answer on %s over %d bytes; not audited", row.Name, route, limit)
			}
			return
		}
		now := time.Now().UTC()
		status := c.Writer.Status()
		switch {
		case isAuditedRefusal(status) && sink.cfg.Denied:
			sink.enqueueRefusal(c, route, row, ids, actorRole, status, writer.buf.Bytes(), now)
		case status == http.StatusOK && row.Batch && sink.cfg.Denied:
			sink.enqueueBatchDenials(c, route, row, actorRole, writer.buf.Bytes(), now)
		}
	}
}

// holdAndAuditEffectBound runs the handler with its answer held. A refusal is
// released unchanged (its row best effort); an issued decision is released only
// once its row is in the outbox, otherwise the typed 503 replaces it.
func holdAndAuditEffectBound(c *gin.Context, sink decisionAuditSink, route string, row spaces.DecisionAuditRoute, ids decisionAuditRequestIDs, actorRole string) {
	hold := &decisionAuditHoldWriter{ResponseWriter: c.Writer}
	c.Writer = hold
	defer func() {
		// A panic leaves the real writer to the recovery middleware.
		if recovered := recover(); recovered != nil {
			c.Writer = hold.ResponseWriter
			panic(recovered)
		}
	}()
	c.Next()
	c.Writer = hold.ResponseWriter

	status := hold.Status()
	if c.IsAborted() || status != http.StatusOK {
		hold.release()
		if !c.IsAborted() && isAuditedRefusal(status) && sink.cfg.Denied {
			sink.enqueueRefusal(c, route, row, ids, actorRole, status, hold.buf.Bytes(), time.Now().UTC())
		}
		return
	}
	d, ok := issuedDecisionForAudit(c, hold.buf.Bytes())
	if !ok {
		// Every effect-bound issuer answers 200 only with a decision. A 200 the
		// middleware cannot attribute to one may still carry a token, so it is
		// withheld rather than released unaudited.
		log.Printf("user-core space decision audit: %s 200 on %s carries no auditable decision; withheld (fail closed)", row.Name, route)
		hold.refuse()
		return
	}
	eventID, subject, payload, err := spaces.BuildIssuedDecisionAudit(spaces.IssuedDecisionAudit{
		Route: route, Kind: row.Name, DecisionRef: d.DecisionRef,
		OrgID:     firstNonEmpty(d.OrgID, c.GetString("org_id")),
		SubjectID: firstNonEmpty(d.SubjectID, c.GetString("user_id")),
		SpaceRef:  firstNonEmpty(d.SpaceRef, ids.SpaceRef), ActionID: firstNonEmpty(d.ActionID, ids.ActionID),
		ServiceAudience: d.ServiceAudience, AuthorityRevision: d.AuthorityRevision,
		ExpiresAt: d.ExpiresAt, ZeroDataRetention: d.ZeroDataRetention,
		ActorRole: actorRole, OccurredAt: time.Now().UTC(),
	})
	if !sink.enqueue(c, eventID, subject, payload, err) {
		log.Printf("user-core space decision audit: %s issue on %s withheld (fail closed)", row.Name, route)
		hold.refuse()
		return
	}
	hold.release()
}

func (k decisionAuditSink) enqueueRefusal(c *gin.Context, route string, row spaces.DecisionAuditRoute, ids decisionAuditRequestIDs, actorRole string, status int, body []byte, now time.Time) {
	var answer decisionAuditResponse
	_ = json.Unmarshal(body, &answer)
	code, resolvableBy := answer.refusalCode()
	eventID, subject, payload, err := spaces.BuildDeniedDecisionAudit(spaces.DeniedDecisionAudit{
		Route: route, Kind: row.Name, OrgID: firstNonEmpty(c.GetString("org_id"), ids.OrgID),
		SubjectID: firstNonEmpty(c.GetString("user_id"), ids.SubjectID),
		SpaceRef:  ids.SpaceRef, ActionID: ids.ActionID, Status: status,
		Reason: code, ResolvableBy: resolvableBy, ActorRole: actorRole, OccurredAt: now,
	})
	k.enqueue(c, eventID, subject, payload, err)
}

// enqueueBatchDenials writes one row per denied result row of a 200 batch
// answer. Issued rows are thread reads and, like the single route, never
// audited as issued.
func (k decisionAuditSink) enqueueBatchDenials(c *gin.Context, route string, row spaces.DecisionAuditRoute, actorRole string, body []byte, now time.Time) {
	var answer decisionAuditBatchResponse
	if json.Unmarshal(body, &answer) != nil || answer.Data == nil {
		return
	}
	// One deadline covers the whole batch: a stalled outbox costs the caller at
	// most decisionAuditWriteTimeout, not one timeout per denied row. Rows still
	// unwritten when it expires fail fast and are counted in one log line.
	ctx, cancel := context.WithTimeout(context.WithoutCancel(c.Request.Context()), decisionAuditWriteTimeout)
	defer cancel()
	lost := 0
	for _, result := range answer.Data.Results {
		if result.Status != "denied" {
			continue
		}
		if ctx.Err() != nil {
			lost++
			continue
		}
		eventID, subject, payload, err := spaces.BuildDeniedDecisionAudit(spaces.DeniedDecisionAudit{
			Route: route, Kind: row.Name, OrgID: c.GetString("org_id"), SubjectID: c.GetString("user_id"),
			SpaceRef: result.SpaceRef, Status: http.StatusOK, Reason: result.Reason,
			ActorRole: actorRole, BatchRow: true, OccurredAt: now,
		})
		if !k.enqueueWithin(ctx, eventID, subject, payload, err) {
			lost++
		}
	}
	if lost > 0 {
		log.Printf("user-core space decision audit: %d batch denial rows on %s not recorded", lost, route)
	}
}

// enqueue writes one row and reports whether it is in the outbox.
func (k decisionAuditSink) enqueue(c *gin.Context, eventID, subject string, payload []byte, buildErr error) bool {
	ctx, cancel := context.WithTimeout(context.WithoutCancel(c.Request.Context()), decisionAuditWriteTimeout)
	defer cancel()
	return k.enqueueWithin(ctx, eventID, subject, payload, buildErr)
}

// enqueueWithin writes one row under the caller's deadline.
func (k decisionAuditSink) enqueueWithin(ctx context.Context, eventID, subject string, payload []byte, buildErr error) bool {
	if buildErr != nil {
		log.Printf("user-core space decision audit: not recorded: %v", buildErr)
		return false
	}
	if err := k.outbox.EnqueueAudit(ctx, eventID, subject, payload); err != nil {
		log.Printf("user-core space decision audit: enqueue %s failed: %v", eventID, err)
		return false
	}
	return true
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if value != "" {
			return value
		}
	}
	return ""
}
