package api

import (
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// skillRows serves canned agent_skills rows to the list handler, scanning each
// column the handler selects in order.
type skillRows struct {
	rows [][]any
	next int
}

func (r *skillRows) Close()                                       {}
func (r *skillRows) Err() error                                   { return nil }
func (r *skillRows) CommandTag() pgconn.CommandTag                { return pgconn.CommandTag{} }
func (r *skillRows) FieldDescriptions() []pgconn.FieldDescription { return nil }
func (r *skillRows) Values() ([]any, error)                       { return nil, nil }
func (r *skillRows) RawValues() [][]byte                          { return nil }
func (r *skillRows) Conn() *pgx.Conn                              { return nil }
func (r *skillRows) Next() bool {
	r.next++
	return r.next <= len(r.rows)
}

func (r *skillRows) Scan(dest ...any) error {
	row := r.rows[r.next-1]
	if len(dest) != len(row) {
		return errors.New("skillRows: the handler selects a different number of columns than the fixture has")
	}
	for i, value := range row {
		switch target := dest[i].(type) {
		case *string:
			*target = value.(string)
		case *bool:
			*target = value.(bool)
		case *[]string:
			*target = value.([]string)
		case *time.Time:
			*target = value.(time.Time)
		default:
			return errors.New("skillRows: unsupported destination type")
		}
	}
	return nil
}

// The registry reports where a skill came from and whether its owner has
// decided on it, so the owner's inbox and the Skills page can tell a proposal
// from a skill without a second call.
func TestSkillListReturnsOriginAndReviewState(t *testing.T) {
	now := time.Now().UTC()
	database := &recordingDatabase{rows: &skillRows{rows: [][]any{
		{"sk-1", "org-a", "Weekly report", "d", "steps", []string{"report"}, []string{}, []string{},
			false, "user", "user-a", []string{}, "background_review", "proposed", "run-9", "asked three weeks running", now, now},
		{"sk-2", "org-a", "Runbook", "d", "steps", []string{}, []string{}, []string{},
			true, "org", "", []string{}, "user", "accepted", "", "", now, now},
	}}}
	skills := NewSkillsHandler(database)
	mux := http.NewServeMux()
	skills.Register(mux)
	handler, _, writeToken := mcpAuthenticatedHandler(t, mux)

	request := httptest.NewRequest(http.MethodGet, "/api/v1/skills", nil)
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	var body struct {
		Skills []skillRow `json:"skills"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil || len(body.Skills) != 2 {
		t.Fatalf("body = %s err=%v", response.Body.String(), err)
	}
	proposal, accepted := body.Skills[0], body.Skills[1]
	if proposal.Origin != "background_review" || proposal.ReviewState != "proposed" || proposal.SourceRunID != "run-9" || proposal.ProposalReason != "asked three weeks running" || proposal.Enabled {
		t.Fatalf("proposal fields not reported: %+v", proposal)
	}
	if accepted.Origin != "user" || accepted.ReviewState != "accepted" {
		t.Fatalf("accepted fields not reported: %+v", accepted)
	}
}

// A proposal is its owner's alone: a share written onto it reaches no one, and
// neither does the admin governance view, until it is accepted.
func TestSkillListHidesAProposalFromEveryoneButItsOwner(t *testing.T) {
	database := &recordingDatabase{}
	skills := NewSkillsHandler(database)
	mux := http.NewServeMux()
	skills.Register(mux)
	handler, _, writeToken := mcpAuthenticatedHandler(t, mux)

	request := httptest.NewRequest(http.MethodGet, "/api/v1/skills", nil)
	request.Header.Set("Authorization", "Bearer "+writeToken)
	handler.ServeHTTP(httptest.NewRecorder(), request)

	query := database.queries[len(database.queries)-1].query
	owner := strings.Index(query, "owner_user_id = $2")
	accepted := strings.Index(query, "review_state = 'accepted' AND (shared_with ? $2")
	if owner < 0 || accepted < 0 || accepted < owner {
		t.Fatalf("a share must count only on an accepted skill, and the owner always sees their own:\n%s", query)
	}
}

func TestSkillPatchRefusesToEnableAProposal(t *testing.T) {
	// The guarded UPDATE touches no row; the review state read after it says why.
	database := &recordingDatabase{execTag: "UPDATE 0"}
	database.nextRows = append(database.nextRows, stringRow{value: "proposed"})
	skills := NewSkillsHandler(database)
	mux := http.NewServeMux()
	skills.Register(mux)
	handler, _, writeToken := mcpAuthenticatedHandler(t, mux)

	request := httptest.NewRequest(http.MethodPatch, "/api/v1/skills/sk-1", strings.NewReader(`{"enabled":true}`))
	request.Header.Set("Authorization", "Bearer "+writeToken)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)

	if response.Code != http.StatusConflict || !strings.Contains(response.Body.String(), "proposal_requires_decision") {
		t.Fatalf("status = %d, body=%s, want 409 proposal_requires_decision", response.Code, response.Body.String())
	}
	if !strings.Contains(database.execs[0].query, "review_state = 'accepted'") {
		t.Fatalf("the refusal must be part of the UPDATE itself, not a check beside it:\n%s", database.execs[0].query)
	}
}

func TestSkillPatchStillSwitchesAnAcceptedSkillOffAndOn(t *testing.T) {
	for _, body := range []string{`{"enabled":true}`, `{"enabled":false}`} {
		database := &recordingDatabase{}
		skills := NewSkillsHandler(database)
		mux := http.NewServeMux()
		skills.Register(mux)
		handler, _, writeToken := mcpAuthenticatedHandler(t, mux)

		request := httptest.NewRequest(http.MethodPatch, "/api/v1/skills/sk-1", strings.NewReader(body))
		request.Header.Set("Authorization", "Bearer "+writeToken)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, request)
		if response.Code != http.StatusOK {
			t.Fatalf("%s: status = %d, body=%s, want 200", body, response.Code, response.Body.String())
		}
	}
}
