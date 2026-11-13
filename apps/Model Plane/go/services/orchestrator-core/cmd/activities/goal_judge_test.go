package activities

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"sort"
	"strings"
	"sync"
	"testing"

	"go.temporal.io/sdk/temporal"

	"github.com/triodelab/model-plane/services/orchestrator-core/internal/grpcclient"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/servicecred"
)

// W3-EP-7-S6-b: JudgeGoalTurnActivity posts ids only to model-gateway's judge route with
// orchestrator-core's own credentials, and fails closed.

type judgeMinter struct {
	audience    string
	token       string
	err         error
	mu          sync.Mutex
	orgs        []string
	invalidated []string
}

func (m *judgeMinter) Token(_ context.Context, orgID string) (string, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.orgs = append(m.orgs, orgID)
	return m.token, m.err
}

func (m *judgeMinter) Invalidate(orgID string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.invalidated = append(m.invalidated, orgID)
}

func (m *judgeMinter) Audience() string { return m.audience }

type judgeCall struct {
	method, path, auth, inference, contentType string
	body                                       []byte
}

func judgeServer(t *testing.T, status int) (*httptest.Server, *[]judgeCall) {
	t.Helper()
	var calls []judgeCall
	var mu sync.Mutex
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		mu.Lock()
		calls = append(calls, judgeCall{
			method: r.Method, path: r.URL.EscapedPath(), auth: r.Header.Get("Authorization"),
			inference: r.Header.Get("x-inference-authorization"), contentType: r.Header.Get("Content-Type"), body: body,
		})
		mu.Unlock()
		w.WriteHeader(status)
		_, _ = w.Write([]byte(`{"data":{"verdict":"objective text the reply may carry"}}`))
	}))
	t.Cleanup(server.Close)
	return server, &calls
}

func judgeActivities(baseURL string, gateway, inference *judgeMinter) *Activities {
	return NewActivities(slog.New(slog.NewTextHandler(io.Discard, nil)), &grpcclient.Clients{
		ModelGateway: &grpcclient.ModelGateway{BaseURL: baseURL, Minters: servicecred.Minters{
			servicecred.AudienceModelGateway:  gateway,
			servicecred.AudienceInferenceCore: inference,
		}},
	})
}

func judgeInput() GoalJudgeTurnInput {
	return GoalJudgeTurnInput{GoalID: "goal_01J/../x", RunID: "run-1", OrgID: "org-1", ScheduleID: "sched-1", FireKey: "2026-11-12T09:00:00Z"}
}

func TestJudgeGoalTurnActivityPostsIdsOnly(t *testing.T) {
	server, calls := judgeServer(t, http.StatusOK)
	gateway := &judgeMinter{audience: "model-gateway", token: "gw-token"}
	inference := &judgeMinter{audience: "inference-core", token: "inf-token"}
	if err := judgeActivities(server.URL, gateway, inference).JudgeGoalTurnActivity(context.Background(), judgeInput()); err != nil {
		t.Fatalf("JudgeGoalTurnActivity: %v", err)
	}
	if len(*calls) != 1 {
		t.Fatalf("calls = %d", len(*calls))
	}
	call := (*calls)[0]
	if call.method != http.MethodPost || call.path != "/internal/v1/goals/goal_01J%2F..%2Fx/judge-turn" {
		t.Fatalf("request %s %s; the goal id must be one escaped path segment", call.method, call.path)
	}
	if call.auth != "Bearer gw-token" || call.inference != "Bearer inf-token" || call.contentType != "application/json" {
		t.Fatalf("headers auth=%q inference=%q type=%q", call.auth, call.inference, call.contentType)
	}
	var body map[string]any
	if err := json.Unmarshal(call.body, &body); err != nil {
		t.Fatalf("body: %v (%s)", err, call.body)
	}
	keys := make([]string, 0, len(body))
	for key, value := range body {
		keys = append(keys, key)
		if _, isString := value.(string); !isString {
			t.Fatalf("field %s is not an id string: %v", key, value)
		}
	}
	sort.Strings(keys)
	if strings.Join(keys, ",") != "fire_key,org_id,run_id,schedule_id" {
		t.Fatalf("body keys = %v; want the four ids only (the goal id is in the path)", keys)
	}
	if body["run_id"] != "run-1" || body["org_id"] != "org-1" || body["schedule_id"] != "sched-1" || body["fire_key"] != "2026-11-12T09:00:00Z" {
		t.Fatalf("body = %v", body)
	}
	// Both bearers are minted for the fire's org, never another tenant.
	if gateway.orgs[0] != "org-1" || inference.orgs[0] != "org-1" {
		t.Fatalf("minted for %v / %v", gateway.orgs, inference.orgs)
	}
}

func TestJudgeGoalTurnActivityFailsOnANon2xxAndRefreshesOnA401(t *testing.T) {
	for _, status := range []int{http.StatusUnauthorized, http.StatusForbidden, http.StatusConflict, http.StatusInternalServerError} {
		server, _ := judgeServer(t, status)
		gateway := &judgeMinter{audience: "model-gateway", token: "gw"}
		inference := &judgeMinter{audience: "inference-core", token: "inf"}
		err := judgeActivities(server.URL, gateway, inference).JudgeGoalTurnActivity(context.Background(), judgeInput())
		if err == nil {
			t.Fatalf("status %d: no error", status)
		}
		if strings.Contains(err.Error(), "objective text") {
			t.Fatalf("status %d: the reply body reached the error: %v", status, err)
		}
		invalidated := len(gateway.invalidated) == 1 && len(inference.invalidated) == 1
		if (status == http.StatusUnauthorized) != invalidated {
			t.Fatalf("status %d: invalidated gateway=%v inference=%v", status, gateway.invalidated, inference.invalidated)
		}
	}
}

func TestJudgeGoalTurnActivityRefusesAMissingIdBeforeAnyCall(t *testing.T) {
	server, calls := judgeServer(t, http.StatusOK)
	for _, blank := range []func(*GoalJudgeTurnInput){
		func(in *GoalJudgeTurnInput) { in.GoalID = "" },
		func(in *GoalJudgeTurnInput) { in.RunID = " " },
		func(in *GoalJudgeTurnInput) { in.OrgID = "" },
		func(in *GoalJudgeTurnInput) { in.ScheduleID = "" },
		func(in *GoalJudgeTurnInput) { in.FireKey = "" },
	} {
		in := judgeInput()
		blank(&in)
		err := judgeActivities(server.URL, &judgeMinter{token: "gw"}, &judgeMinter{token: "inf"}).JudgeGoalTurnActivity(context.Background(), in)
		var app *temporal.ApplicationError
		if !errors.As(err, &app) || !app.NonRetryable() {
			t.Fatalf("input %+v: err = %v; want a non-retryable application error", in, err)
		}
	}
	if len(*calls) != 0 {
		t.Fatalf("a blank id reached the gateway (%d calls)", len(*calls))
	}
}

func TestJudgeGoalTurnActivityFailsClosedWithoutTheGatewayOrItsCredentials(t *testing.T) {
	logger := slog.New(slog.NewTextHandler(io.Discard, nil))
	if err := NewActivities(logger, &grpcclient.Clients{}).JudgeGoalTurnActivity(context.Background(), judgeInput()); err == nil {
		t.Fatal("no gateway configured, but the judge reported success")
	}
	if err := NewActivities(logger, nil).JudgeGoalTurnActivity(context.Background(), judgeInput()); err == nil {
		t.Fatal("no clients, but the judge reported success")
	}
	server, calls := judgeServer(t, http.StatusOK)
	noInference := NewActivities(logger, &grpcclient.Clients{ModelGateway: &grpcclient.ModelGateway{
		BaseURL: server.URL, Minters: servicecred.Minters{servicecred.AudienceModelGateway: &judgeMinter{token: "gw"}},
	}})
	if err := noInference.JudgeGoalTurnActivity(context.Background(), judgeInput()); err == nil {
		t.Fatal("no inference bearer, but the judge reported success")
	}
	failingMint := judgeActivities(server.URL, &judgeMinter{err: errors.New("auth down")}, &judgeMinter{token: "inf"})
	if err := failingMint.JudgeGoalTurnActivity(context.Background(), judgeInput()); err == nil {
		t.Fatal("a failed mint reported success")
	}
	if len(*calls) != 0 {
		t.Fatalf("an uncredentialed judge call was sent (%d)", len(*calls))
	}
}
