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
//   - a 200 carrying data.decision on an effect-bound route -> space_decision_issued.
//
// The middleware never changes the response, the status or the decision: the
// writer wrapper only copies the first bytes of the body. The request body is
// read for four identifiers (org, subject, Space, action) and restored. The
// token, payload digest, nonce and recipient references are never read into the
// event. Writing the row is best effort: an outbox failure is logged and never
// turns a refusal into an issue or an issue into a refusal.

const (
	decisionAuditMaxRequestBody  = 1 << 20
	decisionAuditMaxResponseBody = 64 << 10
	decisionAuditWriteTimeout    = 3 * time.Second
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
	truncated bool
}

func (w *decisionAuditWriter) capture(p []byte) {
	if room := decisionAuditMaxResponseBody - w.buf.Len(); room > 0 {
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
	// Typed refusal body.
	Code         string `json:"code"`
	ResolvableBy string `json:"resolvable_by"`
	// Issued body: {"data":{"decision":{...}}}.
	Data *struct {
		Decision *struct {
			DecisionRef       string    `json:"decision_ref"`
			OrgID             string    `json:"org_id"`
			SpaceRef          string    `json:"space_ref"`
			SubjectID         string    `json:"subject_id"`
			ServiceAudience   string    `json:"service_audience"`
			ActionID          string    `json:"action_id"`
			AuthorityRevision int64     `json:"authority_revision"`
			ExpiresAt         time.Time `json:"expires_at"`
			ZeroDataRetention bool      `json:"zero_data_retention"`
		} `json:"decision"`
	} `json:"data"`
}

func (s *Server) spaceDecisionAuditMiddleware() gin.HandlerFunc {
	return func(c *gin.Context) {
		sink := s.spaceDecisionAudit
		if !sink.cfg.Enabled || sink.outbox == nil || c.Request.Method != http.MethodPost {
			c.Next()
			return
		}
		route, row, ok := spaces.DecisionAuditRouteFor(c.Request.URL.Path)
		if !ok || (!sink.cfg.Denied && !(sink.cfg.Issued && row.EffectBound)) {
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

		writer := &decisionAuditWriter{ResponseWriter: c.Writer}
		c.Writer = writer
		c.Next()
		if c.IsAborted() || writer.truncated {
			return
		}

		now := time.Now().UTC()
		status := c.Writer.Status()
		switch {
		case status == http.StatusForbidden && sink.cfg.Denied:
			var answer decisionAuditResponse
			_ = json.Unmarshal(writer.buf.Bytes(), &answer)
			orgID := firstNonEmpty(c.GetString("org_id"), ids.OrgID)
			eventID, subject, payload, err := spaces.BuildDeniedDecisionAudit(spaces.DeniedDecisionAudit{
				Route: route, Kind: row.Name, OrgID: orgID,
				SubjectID: firstNonEmpty(c.GetString("user_id"), ids.SubjectID),
				SpaceRef:  ids.SpaceRef, ActionID: ids.ActionID, Status: status,
				Reason: answer.Code, ResolvableBy: answer.ResolvableBy, OccurredAt: now,
			})
			sink.enqueue(c, eventID, subject, payload, err)
		case status == http.StatusOK && sink.cfg.Issued && row.EffectBound:
			var answer decisionAuditResponse
			if json.Unmarshal(writer.buf.Bytes(), &answer) != nil || answer.Data == nil || answer.Data.Decision == nil {
				return
			}
			d := answer.Data.Decision
			eventID, subject, payload, err := spaces.BuildIssuedDecisionAudit(spaces.IssuedDecisionAudit{
				Route: route, Kind: row.Name, DecisionRef: d.DecisionRef,
				OrgID:     firstNonEmpty(d.OrgID, c.GetString("org_id")),
				SubjectID: firstNonEmpty(d.SubjectID, c.GetString("user_id")),
				SpaceRef:  firstNonEmpty(d.SpaceRef, ids.SpaceRef), ActionID: firstNonEmpty(d.ActionID, ids.ActionID),
				ServiceAudience: d.ServiceAudience, AuthorityRevision: d.AuthorityRevision,
				ExpiresAt: d.ExpiresAt, ZeroDataRetention: d.ZeroDataRetention, OccurredAt: now,
			})
			sink.enqueue(c, eventID, subject, payload, err)
		}
	}
}

func (k decisionAuditSink) enqueue(c *gin.Context, eventID, subject string, payload []byte, buildErr error) {
	if buildErr != nil {
		log.Printf("user-core space decision audit: not recorded: %v", buildErr)
		return
	}
	ctx, cancel := context.WithTimeout(context.WithoutCancel(c.Request.Context()), decisionAuditWriteTimeout)
	defer cancel()
	if err := k.outbox.EnqueueAudit(ctx, eventID, subject, payload); err != nil {
		log.Printf("user-core space decision audit: enqueue %s failed: %v", eventID, err)
	}
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if value != "" {
			return value
		}
	}
	return ""
}
