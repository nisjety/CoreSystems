package activities

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"

	"go.temporal.io/sdk/temporal"

	"github.com/triodelab/model-plane/services/orchestrator-core/internal/grpcclient"
	"github.com/triodelab/model-plane/services/orchestrator-core/internal/servicecred"
)

// GoalJudgeTurnInput is the input of JudgeGoalTurnActivity: the ids of one finished fire of a
// goal-bound schedule (EP-7 S6). It carries ids only, because Temporal history must never hold the
// objective, the criteria or the answer; model-gateway reads those from their owners.
type GoalJudgeTurnInput struct {
	GoalID     string `json:"goal_id"`
	RunID      string `json:"run_id"`
	OrgID      string `json:"org_id"`
	ScheduleID string `json:"schedule_id"`
	FireKey    string `json:"fire_key"`
}

const (
	// goalJudgeTimeout bounds one judge call. It matches the activity's StartToClose timeout, so the
	// HTTP call never outlives the attempt Temporal allows it.
	goalJudgeTimeout = 120 * time.Second

	// maxGoalJudgeReplyBytes bounds how much of the judge's reply is read. A successful reply is a
	// verdict envelope; it is drained, never kept and never logged.
	maxGoalJudgeReplyBytes = 4 << 10

	// goalJudgeInferenceHeader carries the inference-core bearer the judge call spends. It is the
	// header model-gateway's judge route reads.
	goalJudgeInferenceHeader = "x-inference-authorization"
)

// goalJudgeHTTP is shared by every judge call, so its connections are pooled.
var goalJudgeHTTP = &http.Client{Timeout: goalJudgeTimeout}

// goalJudgeRequest is the judge route's body. It is ids only, and the route refuses unknown fields,
// so the goal id travels in the path and nothing else is added.
type goalJudgeRequest struct {
	RunID      string `json:"run_id"`
	OrgID      string `json:"org_id"`
	ScheduleID string `json:"schedule_id"`
	FireKey    string `json:"fire_key"`
}

// JudgeGoalTurnActivity asks model-gateway for the completion verdict on the goal turn one finished
// fire spent (EP-7 S6). The run is complete and its turn ledgered before this runs, so a failure
// only leaves the turn unjudged: the goal's caps still stop it, and the next fire is judged afresh.
// The gateway records the verdict itself; nothing comes back to the workflow but the error.
func (a *Activities) JudgeGoalTurnActivity(ctx context.Context, in GoalJudgeTurnInput) error {
	if a.clients == nil || a.clients.ModelGateway == nil {
		return errors.New("model-gateway goal judge is not configured")
	}
	for _, field := range []struct{ name, value string }{
		{"goal_id", in.GoalID},
		{"run_id", in.RunID},
		{"org_id", in.OrgID},
		{"schedule_id", in.ScheduleID},
		{"fire_key", in.FireKey},
	} {
		if strings.TrimSpace(field.value) == "" {
			return temporal.NewNonRetryableApplicationError(
				fmt.Sprintf("goal judge %s is required", field.name), "GoalJudgeInputInvalid", nil)
		}
	}
	return postGoalJudgeTurn(ctx, a.clients.ModelGateway, in)
}

// postGoalJudgeTurn sends the judge call with orchestrator-core's own credentials: its model-gateway
// token, which holds model:goal_judge pinned to the body's org, and the inference-core bearer the
// call spends. A bearer the gateway refuses is dropped from the minter's cache, so the Temporal retry
// mints a fresh one.
func postGoalJudgeTurn(ctx context.Context, gateway *grpcclient.ModelGateway, in GoalJudgeTurnInput) error {
	gatewayMinter := gateway.Minters.Get(servicecred.AudienceModelGateway)
	inferenceMinter := gateway.Minters.Get(servicecred.AudienceInferenceCore)
	if gatewayMinter == nil || inferenceMinter == nil {
		return errors.New("goal judge service credentials are not configured")
	}
	gatewayToken, err := gatewayMinter.Token(ctx, in.OrgID)
	if err != nil {
		return fmt.Errorf("mint model-gateway judge token: %w", err)
	}
	inferenceToken, err := inferenceMinter.Token(ctx, in.OrgID)
	if err != nil {
		return fmt.Errorf("mint inference-core judge bearer: %w", err)
	}
	body, err := json.Marshal(goalJudgeRequest{
		RunID: in.RunID, OrgID: in.OrgID, ScheduleID: in.ScheduleID, FireKey: in.FireKey,
	})
	if err != nil {
		return fmt.Errorf("encode goal judge request: %w", err)
	}
	endpoint := gateway.BaseURL + "/internal/v1/goals/" + url.PathEscape(in.GoalID) + "/judge-turn"
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("build goal judge request: %w", err)
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("Authorization", "Bearer "+gatewayToken)
	request.Header.Set(goalJudgeInferenceHeader, "Bearer "+inferenceToken)

	response, err := goalJudgeHTTP.Do(request)
	if err != nil {
		return fmt.Errorf("request goal judge: %w", err)
	}
	defer func() { _ = response.Body.Close() }()
	_, _ = io.Copy(io.Discard, io.LimitReader(response.Body, maxGoalJudgeReplyBytes))
	if response.StatusCode == http.StatusUnauthorized {
		gatewayMinter.Invalidate(in.OrgID)
		inferenceMinter.Invalidate(in.OrgID)
	}
	if response.StatusCode < http.StatusOK || response.StatusCode >= http.StatusMultipleChoices {
		return fmt.Errorf("model-gateway refused the goal judge: status %d", response.StatusCode)
	}
	return nil
}
