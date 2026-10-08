package http

import (
	"bytes"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// D12 regression. `bindSpaceDecisionBody` used to take the required values as
// `request.SpaceRef, request.IdempotencyKey, ...` arguments. Go evaluates call
// arguments BEFORE the call, so they were read while `request` was still the
// zero value, before the body had been decoded into it: the check compared empty
// strings and every Space decision endpoint that used it answered 400 to a
// complete, valid body. The live user-service log shows exactly that
// (`thread-read-decision` and `retrieval-decision` -> 400, never a 200), so no
// Space thread-read, retrieval, schedule or import decision was ever issued.
// The required fields are now passed as pointers and read after decoding.

func TestBindSpaceDecisionBodyChecksTheDecodedFieldsNotTheirZeroValues(t *testing.T) {
	type request struct {
		SpaceRef       string `json:"space_ref"`
		IdempotencyKey string `json:"idempotency_key"`
	}
	bind := func(body string) (ok bool, code int) {
		w := httptest.NewRecorder()
		c, _ := gin.CreateTestContext(w)
		c.Request = httptest.NewRequest(http.MethodPost, "/x", bytes.NewBufferString(body))
		c.Request.Header.Set("Content-Type", "application/json")
		var r request
		ok = bindSpaceDecisionBody(c, &r, "space_ref and idempotency_key are required", &r.SpaceRef, &r.IdempotencyKey)
		return ok, w.Code
	}

	if ok, code := bind(`{"space_ref":"space-1","idempotency_key":"key-1"}`); !ok {
		t.Fatalf("a complete body was refused (HTTP %d)", code)
	}
	for name, body := range map[string]string{
		"missing space_ref":       `{"idempotency_key":"key-1"}`,
		"blank space_ref":         `{"space_ref":"  ","idempotency_key":"key-1"}`,
		"missing idempotency_key": `{"space_ref":"space-1"}`,
		"empty object":            `{}`,
		"not json":                `nope`,
	} {
		ok, code := bind(body)
		if ok || code != http.StatusBadRequest {
			t.Fatalf("%s: want a 400 refusal, got ok=%v code=%d", name, ok, code)
		}
	}
}

// Every Space decision handler that validates required body fields must get past
// binding with a complete body. With no database behind the repository the next
// step fails closed (403 or 503), which is the point: anything but 400 proves
// the handler read the decoded body.
func TestSpaceDecisionHandlersAcceptACompleteBody(t *testing.T) {
	handlers := []struct {
		name    string
		handler func(*Server, *gin.Context)
		body    string
	}{
		{"personal thread decision", (*Server).issuePersonalThreadDecision,
			`{"space_ref":"s","session_key":"k","idempotency_key":"i"}`},
		{"thread decision", (*Server).issueThreadDecision,
			`{"space_ref":"s","session_key":"k","idempotency_key":"i"}`},
		{"thread append decision", (*Server).issueThreadAppendDecision,
			`{"space_ref":"s","thread_id":"t","content_digest":"sha256:d","idempotency_key":"i"}`},
		{"thread read decision", (*Server).issueThreadReadDecision,
			`{"space_ref":"s","idempotency_key":"i"}`},
		{"personal retrieval decision", (*Server).issuePersonalRetrievalDecision,
			`{"space_ref":"s","idempotency_key":"i"}`},
		{"retrieval decision", (*Server).issueRetrievalDecision,
			`{"space_ref":"s","idempotency_key":"i"}`},
		{"watch create decision", (*Server).issueWatchCreateDecision,
			`{"space_ref":"s","watch_id":"w","source_kind":"k","source_ref":"r","predicate_digest":"sha256:p","idempotency_key":"i"}`},
		{"watch observe decision", (*Server).issueWatchObserveDecision,
			`{"intent":{"org_id":"o","space_ref":"s","subject_id":"u","watch_id":"w","idempotency_key":"i"}}`},
		{"schedule create decision", (*Server).issueScheduleCreateDecision,
			`{"space_ref":"s","schedule_id":"sc","template_digest":"sha256:t","idempotency_key":"i"}`},
		{"personal import decision", (*Server).issuePersonalImportDecision,
			`{"space_ref":"s","source_type":"upload","idempotency_key":"i"}`},
	}
	for _, tc := range handlers {
		t.Run(tc.name, func(t *testing.T) {
			s := &Server{spaceRepo: &spaces.Repository{}}
			w := httptest.NewRecorder()
			c, _ := gin.CreateTestContext(w)
			c.Request = httptest.NewRequest(http.MethodPost, "/decision", bytes.NewBufferString(tc.body))
			c.Request.Header.Set("Content-Type", "application/json")
			c.Set("org_id", "o")
			c.Set("user_id", "u")
			tc.handler(s, c)
			if w.Code == http.StatusBadRequest {
				t.Fatalf("a complete body was refused as incomplete: %s", w.Body.String())
			}
		})
	}
}
