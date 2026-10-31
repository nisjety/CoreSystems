package api

import (
	"context"
	"errors"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/controlplane/audit-core/internal/store"
)

// exportStatusStub is a compliance store that also knows the exporter's cursors.
type exportStatusStub struct {
	*complianceStub
	status store.ExportStatus
	err    error
	orgs   []string
}

func (s *exportStatusStub) ExportStatus(_ context.Context, orgID string) (store.ExportStatus, error) {
	s.orgs = append(s.orgs, orgID)
	return s.status, s.err
}

func exportStatusRouter(t *testing.T, stub *exportStatusStub) http.Handler {
	t.Helper()
	handler, err := New(stub, complianceTestRegistry())
	if err != nil {
		t.Fatal(err)
	}
	router := chi.NewRouter()
	handler.Mount(router)
	return router
}

func TestComplianceExportStatusReportsTheExportersCursors(t *testing.T) {
	attempt := time.Date(2026, 10, 24, 9, 0, 0, 0, time.UTC)
	stub := &exportStatusStub{complianceStub: &complianceStub{}, status: store.ExportStatus{
		Active:  true,
		HeadSeq: 120,
		Destinations: []store.ExportDestinationStatus{{
			DestinationID: "dest-1", Kind: "ndjson_webhook", CursorSeq: 100, DeliveredTotal: 100,
			ConsecutiveFailures: 2, LastError: "http_503", LastAttemptAt: &attempt,
		}},
	}}
	recorder := complianceGet(exportStatusRouter(t, stub), "/v1/compliance/audit/export-status?org_id=org-a", "org-a", "owner")
	if recorder.Code != http.StatusOK {
		t.Fatalf("status %d: %s", recorder.Code, recorder.Body.String())
	}
	data := decodeEnvelope(t, recorder)["data"].(map[string]any)
	destinations := data["destinations"].([]any)
	if data["exporter_enabled"] != true || data["head_seq"] != float64(120) || len(destinations) != 1 {
		t.Fatalf("export status = %v", data)
	}
	first := destinations[0].(map[string]any)
	if first["cursor_seq"] != float64(100) || first["last_error"] != "http_503" || first["kind"] != "ndjson_webhook" {
		t.Fatalf("destination = %v", first)
	}
	for _, forbidden := range []string{"url", "secret", "org_id"} {
		if _, present := first[forbidden]; present {
			t.Fatalf("destination status carries %q: %v", forbidden, first)
		}
	}
	if len(stub.orgs) != 1 || stub.orgs[0] != "org-a" {
		t.Fatalf("status read for orgs %v, want only the delegated org", stub.orgs)
	}
}

func TestComplianceExportStatusRefusesAMemberWithoutReadingCursors(t *testing.T) {
	stub := &exportStatusStub{complianceStub: &complianceStub{}}
	recorder := complianceGet(exportStatusRouter(t, stub), "/v1/compliance/audit/export-status?org_id=org-a", "org-a", "member")
	if recorder.Code != http.StatusForbidden || len(stub.orgs) != 0 {
		t.Fatalf("status %d, cursor reads %v", recorder.Code, stub.orgs)
	}
}

func TestComplianceExportStatusFailsClosedWhenTheCursorsCannotBeRead(t *testing.T) {
	stub := &exportStatusStub{complianceStub: &complianceStub{}, err: errors.New("relation does not exist")}
	recorder := complianceGet(exportStatusRouter(t, stub), "/v1/compliance/audit/export-status?org_id=org-a", "org-a", "owner")
	if recorder.Code != http.StatusInternalServerError || envelopeErrorCode(t, recorder) != "query_failed" {
		t.Fatalf("status %d: %s", recorder.Code, recorder.Body.String())
	}
	if strings.Contains(recorder.Body.String(), "relation") {
		t.Fatalf("store error text leaked: %s", recorder.Body.String())
	}
	if len(stub.audit) != 0 {
		t.Fatalf("a failed read must not write a self-audit row: %+v", stub.audit)
	}
}
