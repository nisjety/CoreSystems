package api

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/incumbency"
	"github.com/triodelab/anbud/internal/workspace"
)

// fakeWorkspace overrides the one call a test needs to control. Every other method is the real store
// over a nil pool: the tests below refuse those requests before the store reaches the database, so a
// request that did reach it would panic and fail the test.
type fakeWorkspace struct {
	*workspace.Store
	saves   int
	saveErr error
}

// GetProfile reports a profile never saved, which is what an organisation with no profile gets.
func (f *fakeWorkspace) GetProfile(_ context.Context, _ string) (workspace.Profile, error) {
	return workspace.Profile{}, workspace.ErrNotFound
}

func (f *fakeWorkspace) SaveSearch(_ context.Context, _, _, _ string, _ json.RawMessage) (workspace.SavedSearch, error) {
	f.saves++
	return workspace.SavedSearch{}, f.saveErr
}

const workspacePrefix = "/api/v1/workspace"

func workspaceApp(ws Workspace) *fiber.App {
	logger := zerolog.Nop()
	return newServer(ServerConfig{Workspace: ws, Logger: &logger})
}

func jsonTenant(extra ...string) map[string]string {
	return tenant(append([]string{"Content-Type", "application/json"}, extra...)...)
}

func TestWorkspaceRoutesNeedTheKeyAndAnOrganisation(t *testing.T) {
	t.Parallel()

	ws := &fakeWorkspace{Store: workspace.New(nil)}
	app := workspaceApp(ws)
	body := []byte(`{"name":"Brann","query":{"q":"brann"}}`)
	if status, _, _ := send(t, app, http.MethodPost, workspacePrefix+"/searches/write", body, jsonTenant("X-API-Key", "wrong-key")); status != 401 {
		t.Errorf("a wrong key: %d", status)
	}
	status, raw, _ := send(t, app, http.MethodPost, workspacePrefix+"/searches/write", body, jsonTenant("X-Org-ID", ""))
	if status != 400 || decodeError(t, raw).Error.Code != "organization_required" {
		t.Errorf("without an organisation: %d %s", status, raw)
	}
	if ws.saves != 0 {
		t.Errorf("the store was reached by a request that should have been refused")
	}
}

func TestWorkspaceChangesNeedAUser(t *testing.T) {
	t.Parallel()

	app := workspaceApp(&fakeWorkspace{Store: workspace.New(nil)})
	status, raw, _ := send(t, app, http.MethodPost, workspacePrefix+"/pipeline/write",
		[]byte(`{"notice_id":"TED-2026-1","stage":"review"}`), jsonTenant("X-User-ID", ""))
	if status != 401 || decodeError(t, raw).Error.Code != "user_required" {
		t.Errorf("a change with no user: %d %s", status, raw)
	}
}

// The refusal names the rule. It never repeats the value that broke it.
func TestWorkspaceRefusalNamesTheRuleNotTheValue(t *testing.T) {
	t.Parallel()

	app := workspaceApp(&fakeWorkspace{Store: workspace.New(nil)})
	status, raw, _ := send(t, app, http.MethodPost, workspacePrefix+"/pipeline/write",
		[]byte(`{"notice_id":"TED-2026-1","stage":"Lokalprogram-secret"}`), jsonTenant())
	if status != 400 || decodeError(t, raw).Error.Code != "invalid_request" {
		t.Errorf("a stage that is not a pipeline stage: %d %s", status, raw)
	}
	if strings.Contains(raw, "Lokalprogram-secret") {
		t.Errorf("the refusal repeats the value: %s", raw)
	}
}

func TestWorkspaceSavedSearchLimitIsAConflict(t *testing.T) {
	t.Parallel()

	ws := &fakeWorkspace{
		Store:   workspace.New(nil),
		saveErr: fmt.Errorf("%w: at most 50 are kept; delete one first", workspace.ErrTooMany),
	}
	status, raw, _ := send(t, workspaceApp(ws), http.MethodPost, workspacePrefix+"/searches/write",
		[]byte(`{"name":"Brann","query":{"q":"brann"}}`), jsonTenant())
	if status != 409 || decodeError(t, raw).Error.Code != "limit_reached" {
		t.Errorf("the 51st saved search: %d %s", status, raw)
	}
	if ws.saves != 1 {
		t.Errorf("saves = %d, want the store reached once", ws.saves)
	}
}

func TestWorkspaceProfileNeverSavedReadsAsNull(t *testing.T) {
	t.Parallel()

	app := workspaceApp(&fakeWorkspace{Store: workspace.New(nil)})
	status, raw, _ := send(t, app, http.MethodPost, workspacePrefix+"/profile/read", []byte(`{}`), jsonTenant())
	if status != 200 || raw != `{"data":null}` {
		t.Errorf("a profile never saved: %d %s", status, raw)
	}
}

// fakeWins are the awards a suggestion reads.
type fakeWins struct {
	awards []incumbency.Award
	err    error
}

func (f fakeWins) Awards(_ context.Context) ([]incumbency.Award, error) { return f.awards, f.err }

// suggestStore records what a suggestion would save, and reports the profile as created or already saved.
type suggestStore struct {
	*workspace.Store
	created bool
	saves   int
}

func (f *suggestStore) PutProfileIfAbsent(_ context.Context, _, _ string, profile json.RawMessage) (workspace.Profile, bool, error) {
	f.saves++
	return workspace.Profile{Profile: profile}, f.created, nil
}

const suggestOrg = "923456783"

func suggestAward() incumbency.Award {
	return incumbency.Award{
		CPV:     []string{"72212000-7"},
		NUTS:    []string{"NO081"},
		Winners: []eforms.Winner{{Name: "Oss AS", OrgNumber: suggestOrg}},
	}
}

func suggestApp(store *suggestStore, wins WinSource) *fiber.App {
	logger := zerolog.Nop()
	return newServer(ServerConfig{Workspace: store, Wins: wins, Logger: &logger})
}

func TestASuggestionIsSavedWhenTheOrganisationHasNoProfile(t *testing.T) {
	t.Parallel()

	store := &suggestStore{Store: workspace.New(nil), created: true}
	app := suggestApp(store, fakeWins{awards: []incumbency.Award{suggestAward()}})
	status, raw, _ := send(t, app, http.MethodPost, workspacePrefix+"/profile/suggest",
		[]byte(`{"org_number":"923 456 783"}`), jsonTenant())
	if status != 200 {
		t.Fatalf("status = %d %s", status, raw)
	}
	var env struct {
		Data struct {
			Applied bool `json:"applied"`
		} `json:"data"`
	}
	if err := json.Unmarshal([]byte(raw), &env); err != nil || !env.Data.Applied {
		t.Errorf("applied is not true: %s", raw)
	}
	if store.saves != 1 || !strings.Contains(raw, `"cpv":["72212000"]`) {
		t.Errorf("saves = %d, the suggested codes are missing from %s", store.saves, raw)
	}
}

func TestASuggestionNeverReplacesAProfileTheOrganisationSaved(t *testing.T) {
	t.Parallel()

	store := &suggestStore{Store: workspace.New(nil), created: false}
	app := suggestApp(store, fakeWins{awards: []incumbency.Award{suggestAward()}})
	status, raw, _ := send(t, app, http.MethodPost, workspacePrefix+"/profile/suggest",
		[]byte(`{"org_number":"923456783"}`), jsonTenant())
	if status != 200 || !strings.Contains(raw, `"reason":"profile_saved"`) || !strings.Contains(raw, `"applied":false`) {
		t.Errorf("status %d: %s", status, raw)
	}
}

func TestASuggestionWithNoWinsSavesNothing(t *testing.T) {
	t.Parallel()

	store := &suggestStore{Store: workspace.New(nil), created: true}
	app := suggestApp(store, fakeWins{})
	status, raw, _ := send(t, app, http.MethodPost, workspacePrefix+"/profile/suggest",
		[]byte(`{"org_number":"923456783"}`), jsonTenant())
	if status != 200 || !strings.Contains(raw, `"reason":"no_wins"`) || store.saves != 0 {
		t.Errorf("status %d, saves %d: %s", status, store.saves, raw)
	}
}

func TestASuggestionRefusesAnOrganisationNumberThatIsNotValid(t *testing.T) {
	t.Parallel()

	store := &suggestStore{Store: workspace.New(nil), created: true}
	app := suggestApp(store, fakeWins{awards: []incumbency.Award{suggestAward()}})
	status, raw, _ := send(t, app, http.MethodPost, workspacePrefix+"/profile/suggest",
		[]byte(`{"org_number":"123"}`), jsonTenant())
	if status != 400 || decodeError(t, raw).Error.Code != "invalid_request" || store.saves != 0 {
		t.Errorf("status %d, saves %d: %s", status, store.saves, raw)
	}
}

// seenStore records the seen state the tests read and mark.
type seenStore struct {
	*workspace.Store
	marked bool
	by     string
}

func (f *seenStore) Seen(_ context.Context, _ string) (time.Time, bool, error) {
	return time.Date(2026, 10, 28, 9, 0, 0, 0, time.UTC), f.marked, nil
}

func (f *seenStore) MarkSeen(_ context.Context, _, by string) (time.Time, error) {
	f.marked, f.by = true, by
	return time.Date(2026, 10, 28, 10, 0, 0, 0, time.UTC), nil
}

func TestTheSeenStateIsNullUntilMarkedAndThenTheMarkedInstant(t *testing.T) {
	t.Parallel()

	store := &seenStore{Store: workspace.New(nil)}
	logger := zerolog.Nop()
	app := newServer(ServerConfig{Workspace: store, Logger: &logger})

	status, raw, _ := send(t, app, http.MethodPost, workspacePrefix+"/seen/read", []byte(`{}`), jsonTenant())
	if status != 200 || !strings.Contains(raw, `"seen_at":null`) {
		t.Fatalf("read before any mark: %d %s", status, raw)
	}
	status, raw, _ = send(t, app, http.MethodPost, workspacePrefix+"/seen/write", []byte(`{}`), jsonTenant())
	if status != 200 || !strings.Contains(raw, `"seen_at":"2026-10-28T10:00:00Z"`) || store.by != "user-1" {
		t.Errorf("write: %d %s by %q", status, raw, store.by)
	}
	status, raw, _ = send(t, app, http.MethodPost, workspacePrefix+"/seen/read", []byte(`{}`), jsonTenant())
	if status != 200 || !strings.Contains(raw, `"seen_at":"2026-10-28T09:00:00Z"`) {
		t.Errorf("read after a mark: %d %s", status, raw)
	}
}

func TestMarkingSeenNeedsAUser(t *testing.T) {
	t.Parallel()

	store := &seenStore{Store: workspace.New(nil)}
	logger := zerolog.Nop()
	app := newServer(ServerConfig{Workspace: store, Logger: &logger})
	status, raw, _ := send(t, app, http.MethodPost, workspacePrefix+"/seen/write", []byte(`{}`), jsonTenant("X-User-ID", ""))
	if status != 401 || decodeError(t, raw).Error.Code != "user_required" || store.marked {
		t.Errorf("a mark with no user: %d %s", status, raw)
	}
}
