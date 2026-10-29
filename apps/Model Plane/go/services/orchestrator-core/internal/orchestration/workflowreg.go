package orchestration

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"sort"
	"strings"

	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/activities"
	"github.com/triodelab/model-plane/services/orchestrator-core/cmd/workflows"
)

// StartPolicy classifies what a workflow type is allowed to touch, which in
// turn decides who may start it. It exists because the registered workflows are
// not interchangeable: supervising one run is a per-user action, consolidating
// an org's memory is a background maintenance job, and promoting a skill
// rewrites a registry shared beyond the caller's tenant.
type StartPolicy uint8

// Workflow start policies, ordered by how far the blast radius reaches.
const (
	// PolicyRunScoped covers workflows whose effects stay inside one run of one
	// organization. Interactive user principals may start these.
	PolicyRunScoped StartPolicy = iota + 1
	// PolicyScheduledRun is a narrower service-only lane for a Space cron fire.
	// It is distinct from a generic run-scoped workflow because the input names a
	// pre-authorized service-owned thread; Capability Core is the only caller
	// allowed to introduce that thread/run correlation to Temporal.
	PolicyScheduledRun
	// PolicyMaintenance covers org-wide background jobs. Only service
	// principals may start these — a signed-in user has no business kicking off
	// their whole organization's nightly job.
	PolicyMaintenance
	// PolicyGlobalRegistry covers workflows that mutate state shared beyond the
	// caller's organization (the skill registry). These need the dedicated
	// global scope on top of the ordinary start scope.
	PolicyGlobalRegistry
)

// Tenancy is the server-authoritative identity stamped onto every workflow
// input. Every field is derived from the verified caller (and the validated
// run id), never copied out of the request body, so a caller cannot start work
// in another organization by hand-crafting `input`.
type Tenancy struct {
	OrgID  string
	UserID string
	RunID  string
	// ZDR is the caller's signed zero-data-retention posture. Workflow inputs
	// that carry a retention flag receive it so the posture propagates into the
	// durable path instead of being silently dropped.
	ZDR bool
	// RetentionAttested reports whether the issuer stamped a retention posture
	// at all. Without it `ZDR: false` is ambiguous — it is the value both for
	// "declared retainable" and for "nobody declared anything" — and the
	// lifecycle envelope would publish an unearned durability guarantee.
	RetentionAttested bool
}

// Retention maps the caller's signed posture onto the marker the lifecycle
// activities stamp on run events. An unattested caller yields
// [activities.RetentionUnspecified], which fails closed downstream rather than
// asserting that the run's content may be retained.
func (t Tenancy) Retention() activities.Retention {
	if !t.RetentionAttested {
		return activities.RetentionUnspecified
	}
	return activities.RetentionFor(t.ZDR)
}

// WorkflowSpec is one entry of the start allowlist.
//
// Type is the canonical Temporal workflow type — the exact name the worker
// registered. StartWorkflow always hands Temporal this value, never the string
// the client sent, so an unvetted name can never reach the task queue even if
// the lookup were ever loosened.
type WorkflowSpec struct {
	Type          string
	Policy        StartPolicy
	RequiresRunID bool
	// DeniesZDR marks workflows that persist derived content outliving the run
	// (consolidated memory, promoted skills). A zero-data-retention caller is
	// refused rather than quietly having its posture ignored.
	DeniesZDR bool
	// BuildInput decodes the caller-supplied input and returns the typed
	// workflow argument with tenancy stamped in. Unknown fields are rejected so
	// a typo fails loudly instead of silently defaulting.
	BuildInput func(raw json.RawMessage, t Tenancy) (any, error)
}

// ErrUnknownWorkflowType is returned when a requested type is not allowlisted.
var ErrUnknownWorkflowType = errors.New("orchestration: workflow type is not allowlisted")

// workflowAllowlist is the single source of truth for what StartWorkflow can
// start. It must stay in lockstep with the RegisterWorkflow calls in
// cmd/main.go: a name here that the worker does not register would accept a
// start that then sits unhandled on the task queue.
//
// AutoresearchWorkflow is deliberately absent. See cmd/workflows/autoresearch.go
// for why: its budget guard is driven by a fabricated per-step cost because
// ExecuteStepResponse carries no usage data, so exposing it would let callers
// believe a spending cap is enforced when it is not.
var workflowAllowlist = map[string]WorkflowSpec{
	"InteractiveRunSupervision": {
		Type:          "InteractiveRunSupervision",
		Policy:        PolicyRunScoped,
		RequiresRunID: true,
		BuildInput:    buildInteractiveRunInput,
	},
	"ScheduledRunSupervision": {
		Type:          "ScheduledRunSupervision",
		Policy:        PolicyScheduledRun,
		RequiresRunID: true,
		DeniesZDR:     true,
		BuildInput:    buildScheduledRunInput,
	},
	"DeepTaskWorkflow": {
		Type:          "DeepTaskWorkflow",
		Policy:        PolicyRunScoped,
		RequiresRunID: true,
		BuildInput:    buildDeepTaskInput,
	},
	"WideResearchWorkflow": {
		Type:          "WideResearchWorkflow",
		Policy:        PolicyRunScoped,
		RequiresRunID: true,
		BuildInput:    buildWideResearchInput,
	},
	"EvaluatorOptimizerWorkflow": {
		Type:          "EvaluatorOptimizerWorkflow",
		Policy:        PolicyRunScoped,
		RequiresRunID: true,
		BuildInput:    buildEvaluatorOptimizerInput,
	},
	"MemoryConsolidationWorkflow": {
		Type:          "MemoryConsolidationWorkflow",
		Policy:        PolicyMaintenance,
		RequiresRunID: false,
		DeniesZDR:     true,
		BuildInput:    buildMemoryConsolidationInput,
	},
	"SkillPromotionWorkflow": {
		Type:          "SkillPromotionWorkflow",
		Policy:        PolicyGlobalRegistry,
		RequiresRunID: false,
		DeniesZDR:     true,
		BuildInput:    buildSkillPromotionInput,
	},
	"FeedbackPromotionWorkflow": {
		Type:          "FeedbackPromotionWorkflow",
		Policy:        PolicyGlobalRegistry,
		RequiresRunID: false,
		DeniesZDR:     true,
		BuildInput:    buildFeedbackPromotionInput,
	},
}

// LookupWorkflow resolves an allowlisted workflow type. The comparison is
// exact: no case folding, no trimming, no prefix matching.
func LookupWorkflow(name string) (WorkflowSpec, bool) {
	spec, ok := workflowAllowlist[name]
	return spec, ok
}

// AllowedWorkflowTypes returns the allowlisted type names, sorted. Used by the
// registration-parity test and by the error surfaced to a caller that asked for
// an unknown type.
func AllowedWorkflowTypes() []string {
	names := make([]string, 0, len(workflowAllowlist))
	for name := range workflowAllowlist {
		names = append(names, name)
	}
	sort.Strings(names)
	return names
}

// ValidateRunID rejects run ids that cannot round-trip through the NATS subject
// space. Run lifecycle events are published to mp.v1.run.<run_id>.event and the
// downstream learning consumer subscribes to mp.v1.run.*.event, where `*`
// matches exactly ONE token — so a run id containing a dot would publish
// RUN_COMPLETED to a subject nothing is listening on and silently break the
// learning loop. Wildcards are refused for the same reason in reverse.
func ValidateRunID(runID string) error {
	if strings.TrimSpace(runID) == "" {
		return errors.New("orchestration: run_id is required")
	}
	if runID != strings.TrimSpace(runID) {
		return errors.New("orchestration: run_id must not have surrounding whitespace")
	}
	if strings.ContainsAny(runID, ". \t\r\n*>") {
		return errors.New("orchestration: run_id must be a single NATS subject token (no '.', whitespace, '*' or '>')")
	}
	return nil
}

// decodeInput unmarshals caller input strictly. A nil/empty payload decodes as
// the zero value so callers can omit `input` for workflows that need nothing
// beyond tenancy.
func decodeInput(raw json.RawMessage, target any) error {
	trimmed := bytes.TrimSpace(raw)
	if len(trimmed) == 0 || bytes.Equal(trimmed, []byte("null")) {
		return nil
	}
	dec := json.NewDecoder(bytes.NewReader(trimmed))
	dec.DisallowUnknownFields()
	if err := dec.Decode(target); err != nil {
		return fmt.Errorf("orchestration: decode workflow input: %w", err)
	}
	return nil
}

func buildInteractiveRunInput(raw json.RawMessage, t Tenancy) (any, error) {
	var in workflows.InteractiveRunInput
	if err := decodeInput(raw, &in); err != nil {
		return nil, err
	}
	if strings.TrimSpace(in.Goal) == "" {
		return nil, errors.New("orchestration: InteractiveRunSupervision requires a non-empty goal")
	}
	// Tenancy and run identity are server-owned. ThreadID defaults to the run so
	// a single-turn run still has a conversation anchor.
	in.RunID = t.RunID
	in.OrgID = t.OrgID
	in.UserID = t.UserID
	// Server-owned, exactly like the identity fields above: the signed posture
	// always overwrites whatever the caller put in the body, so a client cannot
	// declare its own run retainable.
	in.Retention = t.Retention()
	if strings.TrimSpace(in.ThreadID) == "" {
		in.ThreadID = t.RunID
	}
	return in, nil
}

type scheduledRunRequest struct {
	ThreadID         string `json:"thread_id"`
	TaskTemplateJSON string `json:"task_template_json"`
	SpaceRef         string `json:"space_ref"`
	SubjectID        string `json:"subject_id"`
	ScheduleID       string `json:"schedule_id"`
	FireKey          string `json:"fire_key"`
	TemplateDigest   string `json:"template_digest"`
	IdempotencyKey   string `json:"idempotency_key"`
}

func buildScheduledRunInput(raw json.RawMessage, t Tenancy) (any, error) {
	var req scheduledRunRequest
	if err := decodeInput(raw, &req); err != nil {
		return nil, err
	}
	if t.UserID != "" {
		return nil, errors.New("orchestration: scheduled runs require a service caller")
	}
	for label, value := range map[string]string{
		"thread_id": req.ThreadID, "task_template_json": req.TaskTemplateJSON, "space_ref": req.SpaceRef,
		"subject_id": req.SubjectID, "schedule_id": req.ScheduleID, "fire_key": req.FireKey,
		"template_digest": req.TemplateDigest, "idempotency_key": req.IdempotencyKey,
	} {
		if strings.TrimSpace(value) == "" {
			return nil, fmt.Errorf("orchestration: ScheduledRunSupervision requires %s", label)
		}
	}
	for _, value := range []string{req.ScheduleID, req.FireKey} {
		if strings.ContainsAny(value, "/. \t\r\n*>") {
			return nil, errors.New("orchestration: scheduled-run identifiers are invalid")
		}
	}
	goal, policy, err := scheduledTemplateGoalAndPolicy(req.TaskTemplateJSON, req.TemplateDigest)
	if err != nil {
		return nil, fmt.Errorf("orchestration: scheduled run template is not bound to its execution input: %w", err)
	}
	return workflows.ScheduledRunInput{
		RunID:          t.RunID,
		ThreadID:       strings.TrimSpace(req.ThreadID),
		Goal:           goal,
		Policy:         policy,
		OrgID:          t.OrgID,
		SpaceRef:       strings.TrimSpace(req.SpaceRef),
		SubjectID:      strings.TrimSpace(req.SubjectID),
		ScheduleID:     strings.TrimSpace(req.ScheduleID),
		FireKey:        strings.TrimSpace(req.FireKey),
		TemplateDigest: strings.TrimSpace(req.TemplateDigest),
		PolicyDigest:   fmt.Sprintf("sha256:%x", sha256.Sum256([]byte(policy))),
		IdempotencyKey: strings.TrimSpace(req.IdempotencyKey),
		Retention:      t.Retention(),
	}, nil
}

// scheduledTemplateGoalAndPolicy verifies the immutable task template against
// the Control-bound digest, then derives the only two executable fields the
// scheduled workflow accepts. Goal and policy must never be trusted as
// independent Temporal input: they are the workload Control authorized by way
// of the template commitment.
func scheduledTemplateGoalAndPolicy(templateJSON, expectedDigest string) (string, string, error) {
	templateJSON = strings.TrimSpace(templateJSON)
	expectedDigest = strings.TrimSpace(expectedDigest)
	if templateJSON == "" || expectedDigest == "" || len(templateJSON) > 64<<10 {
		return "", "", errors.New("template binding is invalid")
	}
	var rawTemplate map[string]json.RawMessage
	decoder := json.NewDecoder(strings.NewReader(templateJSON))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&rawTemplate); err != nil || rawTemplate == nil || decoder.Decode(&struct{}{}) != io.EOF {
		return "", "", errors.New("template is not a JSON object")
	}
	canonical, err := json.Marshal(rawTemplate)
	if err != nil || fmt.Sprintf("sha256:%x", sha256.Sum256(canonical)) != expectedDigest {
		return "", "", errors.New("template digest mismatch")
	}
	var template struct {
		Title         string          `json:"title"`
		Description   string          `json:"description"`
		Policy        string          `json:"policy"`
		WorkflowInput json.RawMessage `json:"workflow_input"`
	}
	if err := json.Unmarshal(canonical, &template); err != nil {
		return "", "", errors.New("template is invalid")
	}
	if len(bytes.TrimSpace(template.WorkflowInput)) > 0 {
		var workflowInput map[string]json.RawMessage
		if err := json.Unmarshal(template.WorkflowInput, &workflowInput); err != nil || workflowInput == nil {
			return "", "", errors.New("workflow input is invalid")
		}
		var goal, policy string
		goalRaw, ok := workflowInput["goal"]
		if !ok || json.Unmarshal(goalRaw, &goal) != nil || strings.TrimSpace(goal) == "" {
			return "", "", errors.New("workflow input goal is required")
		}
		if policyRaw, present := workflowInput["policy"]; present && json.Unmarshal(policyRaw, &policy) != nil {
			return "", "", errors.New("workflow input policy is invalid")
		}
		return strings.TrimSpace(goal), strings.TrimSpace(policy), nil
	}
	goal := strings.TrimSpace(template.Title)
	if description := strings.TrimSpace(template.Description); description != "" {
		if goal != "" {
			goal += "\n\n"
		}
		goal += description
	}
	if goal == "" {
		return "", "", errors.New("template has no goal")
	}
	return goal, strings.TrimSpace(template.Policy), nil
}

func buildDeepTaskInput(raw json.RawMessage, t Tenancy) (any, error) {
	var in workflows.DeepTaskInput
	if err := decodeInput(raw, &in); err != nil {
		return nil, err
	}
	if len(in.Steps) == 0 {
		return nil, errors.New("orchestration: DeepTaskWorkflow requires at least one step")
	}
	for i, step := range in.Steps {
		if strings.TrimSpace(step.Goal) == "" {
			return nil, fmt.Errorf("orchestration: DeepTaskWorkflow step %d has an empty goal", i)
		}
	}
	in.ParentRunID = t.RunID
	in.OrgID = t.OrgID
	in.UserID = t.UserID
	return in, nil
}

func buildWideResearchInput(raw json.RawMessage, t Tenancy) (any, error) {
	var in workflows.WideResearchInput
	if err := decodeInput(raw, &in); err != nil {
		return nil, err
	}
	if len(in.Queries) == 0 {
		return nil, errors.New("orchestration: WideResearchWorkflow requires at least one query")
	}
	for i, q := range in.Queries {
		if strings.TrimSpace(q) == "" {
			return nil, fmt.Errorf("orchestration: WideResearchWorkflow query %d is empty", i)
		}
	}
	in.OrgID = t.OrgID
	in.UserID = t.UserID
	return in, nil
}

// evalOptimizerRequest is an explicit snake_case wire contract for
// EvaluatorOptimizerWorkflow. activities.EvalOptimizerInput carries no JSON
// tags, so decoding straight into it would silently depend on Go field names;
// this DTO pins the wire shape instead.
type evalOptimizerRequest struct {
	ThreadID string `json:"thread_id"`

	GeneratorModel  string `json:"generator_model"`
	GeneratorSystem string `json:"generator_system"`
	Task            string `json:"task"`

	JudgeModel  string `json:"judge_model"`
	JudgeSystem string `json:"judge_system"`
	Rubric      string `json:"rubric"`

	MaxRounds           int     `json:"max_rounds"`
	PassThreshold       float64 `json:"pass_threshold"`
	PerRoundTokenBudget int     `json:"per_round_token_budget"`
	TotalTokenBudget    int     `json:"total_token_budget"`
	GeneratorMaxTokens  int     `json:"generator_max_tokens"`
	JudgeMaxTokens      int     `json:"judge_max_tokens"`
	Temperature         float32 `json:"temperature"`
}

func buildEvaluatorOptimizerInput(raw json.RawMessage, t Tenancy) (any, error) {
	var req evalOptimizerRequest
	if err := decodeInput(raw, &req); err != nil {
		return nil, err
	}
	if strings.TrimSpace(req.Task) == "" {
		return nil, errors.New("orchestration: EvaluatorOptimizerWorkflow requires a task")
	}
	threadID := strings.TrimSpace(req.ThreadID)
	if threadID == "" {
		threadID = t.RunID
	}
	return activities.EvalOptimizerInput{
		RunID:               t.RunID,
		ThreadID:            threadID,
		OrgID:               t.OrgID,
		UserID:              t.UserID,
		GeneratorModel:      req.GeneratorModel,
		GeneratorSystem:     req.GeneratorSystem,
		Task:                req.Task,
		JudgeModel:          req.JudgeModel,
		JudgeSystem:         req.JudgeSystem,
		Rubric:              req.Rubric,
		MaxRounds:           req.MaxRounds,
		PassThreshold:       req.PassThreshold,
		PerRoundTokenBudget: req.PerRoundTokenBudget,
		TotalTokenBudget:    req.TotalTokenBudget,
		GeneratorMaxTokens:  req.GeneratorMaxTokens,
		JudgeMaxTokens:      req.JudgeMaxTokens,
		Temperature:         req.Temperature,
		// The signed posture wins; the caller cannot hand us a ZDR flag.
		ZDR:               t.ZDR,
		RetentionAttested: t.RetentionAttested,
	}, nil
}

func buildMemoryConsolidationInput(raw json.RawMessage, t Tenancy) (any, error) {
	var in workflows.MemoryConsolidationInput
	if err := decodeInput(raw, &in); err != nil {
		return nil, err
	}
	in.OrgID = t.OrgID
	return in, nil
}

func buildSkillPromotionInput(raw json.RawMessage, t Tenancy) (any, error) {
	var in workflows.SkillPromotionInput
	if err := decodeInput(raw, &in); err != nil {
		return nil, err
	}
	if strings.TrimSpace(in.SkillID) == "" {
		return nil, errors.New("orchestration: SkillPromotionWorkflow requires a skill_id")
	}
	if strings.TrimSpace(in.ToScope) == "" {
		return nil, errors.New("orchestration: SkillPromotionWorkflow requires a to_scope")
	}
	// The verified caller's tenant, not the request body's: capability-core
	// authorizes the promotion purely by the credential this org mints, so a
	// body-supplied org would be a cross-tenant promotion primitive.
	in.OrgID = t.OrgID
	return in, nil
}

func buildFeedbackPromotionInput(raw json.RawMessage, t Tenancy) (any, error) {
	var in workflows.FeedbackPromotionInput
	if err := decodeInput(raw, &in); err != nil {
		return nil, err
	}
	in.OrgID = t.OrgID
	return in, nil
}
