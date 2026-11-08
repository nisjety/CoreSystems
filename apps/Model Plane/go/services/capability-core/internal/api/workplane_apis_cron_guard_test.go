package api

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	"github.com/triodelab/model-plane/pkg/authctx"
)

// guardScheduleDB answers a schedule's stored template and records every write, so a
// refused re-enable shows as the absence of an UPDATE.
type guardScheduleDB struct {
	template []byte
	writes   []string
}

func (db *guardScheduleDB) Exec(_ context.Context, sql string, _ ...any) (pgconn.CommandTag, error) {
	db.writes = append(db.writes, sql)
	return pgconn.NewCommandTag("UPDATE 1"), nil
}

func (db *guardScheduleDB) Query(context.Context, string, ...any) (pgx.Rows, error) {
	return nil, errors.New("query not expected")
}

func (db *guardScheduleDB) QueryRow(context.Context, string, ...any) pgx.Row {
	return guardTemplateRow{template: db.template}
}

type guardTemplateRow struct{ template []byte }

func (row guardTemplateRow) Scan(dest ...any) error {
	*dest[0].(*[]byte) = row.template
	return nil
}

// guardApprovalTemplate is a schedule template whose approval expires at the given
// instant, written the way WS20-S4 stores it.
func guardApprovalTemplate(t *testing.T, expires time.Time) []byte {
	t.Helper()
	contract := `{"expires_at":"` + expires.UTC().Format("2006-01-02T15:04:05Z") + `","max_rounds":6,"max_tokens_per_round":2048,"tools":[{"approved_at":"2026-10-24T10:00:00Z","capability":"cap.tool.web_search","class":"read","tool":"web_search"}],"v":1}`
	raw, err := json.Marshal(map[string]any{"kind": "cron", "description": "summarise the week", "approval_contract": contract})
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

// A bare re-enable never revives a lapsed approval. Renewal is a new create decision, so
// the PATCH is refused and the schedule is not written.
func TestBareEnableOfExpiredScheduleRefused(t *testing.T) {
	now := time.Now().UTC()
	expired := guardApprovalTemplate(t, now.Add(-time.Hour))
	live := guardApprovalTemplate(t, now.AddDate(0, 0, 30))
	cases := []struct {
		name       string
		guards     bool
		template   []byte
		body       string
		wantStatus int
		wantWrite  bool
	}{
		{name: "re-enabling an expired approval is refused", guards: true, template: expired, body: `{"enabled":true}`, wantStatus: http.StatusConflict},
		{name: "pausing an expired approval is allowed", guards: true, template: expired, body: `{"enabled":false}`, wantStatus: http.StatusOK, wantWrite: true},
		{name: "re-enabling a live approval is allowed", guards: true, template: live, body: `{"enabled":true}`, wantStatus: http.StatusOK, wantWrite: true},
		{name: "re-enabling a draft-only schedule is allowed", guards: true, template: []byte(`{"kind":"cron"}`), body: `{"enabled":true}`, wantStatus: http.StatusOK, wantWrite: true},
		{name: "with the guards off an expired approval resumes as before", guards: false, template: expired, body: `{"enabled":true}`, wantStatus: http.StatusOK, wantWrite: true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			db := &guardScheduleDB{template: tc.template}
			handler := NewCronHandler(db).WithContractGuards(tc.guards)
			request := httptest.NewRequest(http.MethodPatch, "/api/v1/cron/sched-1", strings.NewReader(tc.body))
			request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), authctx.Principal{
				OrganizationID: "org-1",
				ActorID:        "user-1",
				PrincipalType:  "user",
			}))
			recorder := httptest.NewRecorder()
			handler.patch(recorder, request, "sched-1")
			if recorder.Code != tc.wantStatus {
				t.Fatalf("status = %d, want %d: %s", recorder.Code, tc.wantStatus, recorder.Body.String())
			}
			if wrote := len(db.writes) == 1; wrote != tc.wantWrite {
				t.Fatalf("schedule written = %v, want %v (writes %q)", wrote, tc.wantWrite, db.writes)
			}
		})
	}
}
