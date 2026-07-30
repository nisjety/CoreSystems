package taskexec

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"google.golang.org/grpc"
	"google.golang.org/grpc/metadata"
	"google.golang.org/protobuf/types/known/structpb"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/publisher"
	"github.com/triodelab/model-plane/services/capability-core/internal/reconcile"
)

// InternalTokenMetadataKey is the gRPC metadata key carrying the Model-Plane-
// local shared secret orchestrator-core accepts on StartWorkflow. It must match
// orchestration.InternalTokenMetadataKey on the other side; the constant is
// duplicated rather than imported because the two services are separate Go
// modules with no shared contract package.
const InternalTokenMetadataKey = "x-model-plane-internal-token"

// DefaultWorkflowType is the workflow a fired task runs when its template does
// not name one. InteractiveRunSupervision is the run-supervision envelope: it
// starts the run, drives the step loop, and — the reason this whole path
// exists — calls CompleteRunActivity, the only production emitter of
// RUN_COMPLETED.
const DefaultWorkflowType = "InteractiveRunSupervision"

// dispatchActor identifies this component in task_events.actor.
const dispatchActor = "capability-core/task-executor"

// WorkflowStarter is the slice of orchestrator-core's workflow API the
// dispatcher needs. The generated mpv1.OrchestratorWorkflowServiceClient
// satisfies it; tests inject a fake.
type WorkflowStarter interface {
	StartWorkflow(
		ctx context.Context,
		in *mpv1.StartWorkflowRequest,
		opts ...grpc.CallOption,
	) (*mpv1.StartWorkflowResponse, error)
}

// WorkflowDispatcher turns a claimed task into one durable Temporal run.
//
// Why this and not a NATS consumer of mp.v1.capability.task.dispatched: the
// event hand-off has a hole the executor's own comment named — publish succeeds,
// nothing consumes, and the task sits in `running` forever. Starting the
// workflow in-process makes success and failure synchronous with the claim, so
// the Executor's existing "dispatch error ⇒ failed" path is the whole story.
// The reconcile event is still emitted after a successful start, so observers on
// the documented subject keep seeing dispatches; it is now a notification about
// work already started rather than a request for someone to start it.
//
// Idempotency comes from orchestrator-core: the workflow id is derived from
// (workflow type, org, run id) and started with USE_EXISTING, so a redelivered
// or retried dispatch of the same task attaches to the run it already started
// instead of creating a second one.
type WorkflowDispatcher struct {
	pool                *pgxpool.Pool
	client              WorkflowStarter
	internalToken       string
	pub                 publisher.EventPublisher
	defaultWorkflowType string
}

// NewWorkflowDispatcher builds the dispatcher. pub may be nil (reconcile events
// then no-op). internalToken is required: without a credential every start would
// be refused, so an empty token is a configuration error the caller must catch
// before enabling the executor.
func NewWorkflowDispatcher(
	pool *pgxpool.Pool,
	client WorkflowStarter,
	internalToken string,
	pub publisher.EventPublisher,
	defaultWorkflowType string,
) (*WorkflowDispatcher, error) {
	if pool == nil {
		return nil, errors.New("taskexec: workflow dispatcher requires a database pool")
	}
	if client == nil {
		return nil, errors.New("taskexec: workflow dispatcher requires an orchestrator client")
	}
	if strings.TrimSpace(internalToken) == "" {
		return nil, errors.New("taskexec: workflow dispatcher requires an internal service token")
	}
	if strings.TrimSpace(defaultWorkflowType) == "" {
		defaultWorkflowType = DefaultWorkflowType
	}
	return &WorkflowDispatcher{
		pool:                pool,
		client:              client,
		internalToken:       strings.TrimSpace(internalToken),
		pub:                 pub,
		defaultWorkflowType: strings.TrimSpace(defaultWorkflowType),
	}, nil
}

// taskDetail is the claimed row's dispatchable content.
type taskDetail struct {
	title       string
	description string
	orgID       string
	status      string
	config      json.RawMessage
}

// taskTemplate is the optional per-task override carried in tasks.config_json.
// A template naming a workflow type is still validated server-side against
// orchestrator-core's allowlist — this struct only decides what to ask for.
type taskTemplate struct {
	WorkflowType  string          `json:"workflow_type"`
	WorkflowInput json.RawMessage `json:"workflow_input"`
	Policy        string          `json:"policy"`
}

// Dispatch starts the durable run for one claimed task.
//
// A returned error is the Executor's signal to move the task to `failed` with a
// recorded reason, so every failure mode here is either retried by the caller's
// next sweep or terminal-with-an-explanation — never a silent stall in
// `running`.
func (d *WorkflowDispatcher) Dispatch(ctx context.Context, task TaskRef) error {
	detail, err := d.loadTask(ctx, task.ID)
	if err != nil {
		return err
	}
	req, err := dispatchPlan(task, detail, d.defaultWorkflowType)
	if err != nil {
		return err
	}
	if req == nil {
		// Nothing to dispatch (the row left `running` under us) and nothing to
		// fail — another actor owns its state now.
		return nil
	}
	workflowType := req.GetWorkflowType()

	resp, err := d.client.StartWorkflow(d.authorize(ctx), req)
	if err != nil {
		return fmt.Errorf("taskexec: start %s for task %s: %w", workflowType, task.ID, err)
	}

	// Record the linkage before returning so an operator can always answer
	// "which workflow is this task running?".
	//
	// The workflow id goes into task_events, not tasks.run_id: that column is a
	// foreign key into session-core's runs table, and the run row is created by
	// the workflow itself, so writing it here would either violate the
	// constraint or race the workflow.
	if rerr := d.recordStarted(ctx, task, workflowType, resp); rerr != nil {
		// The run IS started. Losing the audit row is worth a loud log, but
		// returning it would mark a live run's task `failed` — a worse lie.
		slog.Warn("task dispatch audit row failed",
			"task", task.ID, "workflow_id", resp.GetWorkflowId(), "error", rerr)
	}

	// Notify observers on the documented subject that a dispatch happened.
	// Best-effort: a bus hiccup must not fail a run that is already going.
	_ = reconcile.Emit(ctx, d.pub, reconcile.KindTask, reconcile.ActionDispatched, task.ID, task.OrgID)
	return nil
}

// authorize attaches the Model-Plane-local internal credential. There is no
// live per-user bearer on a cron-fired path, which is exactly the case
// orchestrator-core's org-bound internal credential exists for.
func (d *WorkflowDispatcher) authorize(ctx context.Context) context.Context {
	return metadata.NewOutgoingContext(ctx, metadata.Pairs(InternalTokenMetadataKey, d.internalToken))
}

func (d *WorkflowDispatcher) loadTask(ctx context.Context, taskID string) (taskDetail, error) {
	var detail taskDetail
	var config []byte
	err := d.pool.QueryRow(ctx, `
		SELECT title, description, org_id, status, config_json
		FROM tasks
		WHERE id = $1 AND deleted_at IS NULL
	`, taskID).Scan(&detail.title, &detail.description, &detail.orgID, &detail.status, &config)
	if errors.Is(err, pgx.ErrNoRows) {
		return taskDetail{}, fmt.Errorf("taskexec: task %s not found", taskID)
	}
	if err != nil {
		return taskDetail{}, fmt.Errorf("taskexec: load task %s: %w", taskID, err)
	}
	detail.config = config
	return detail, nil
}

// dispatchPlan turns a claimed row into the StartWorkflow request to send, or
// (nil, nil) when there is nothing left to dispatch.
//
// Kept free of I/O so every decision it makes — tenant containment, lifecycle
// guard, template handling, goal derivation — is unit-testable without a
// database or an orchestrator.
func dispatchPlan(task TaskRef, detail taskDetail, defaultWorkflowType string) (*mpv1.StartWorkflowRequest, error) {
	// Defense in depth: the claim already selected this row, but a task must
	// never be started under an org other than the one that owns it.
	if detail.orgID != task.OrgID {
		return nil, fmt.Errorf("taskexec: task %s org mismatch (row %q, claim %q)", task.ID, detail.orgID, task.OrgID)
	}
	if detail.orgID == "" {
		return nil, fmt.Errorf("taskexec: task %s has no org_id; refusing to start untenanted work", task.ID)
	}
	if detail.status != "running" {
		return nil, nil
	}

	var tpl taskTemplate
	if len(detail.config) > 0 {
		// A malformed template must not silently become a default run.
		if err := json.Unmarshal(detail.config, &tpl); err != nil {
			return nil, fmt.Errorf("taskexec: task %s config_json is not a valid template: %w", task.ID, err)
		}
	}

	workflowType := strings.TrimSpace(tpl.WorkflowType)
	if workflowType == "" {
		workflowType = strings.TrimSpace(defaultWorkflowType)
	}
	if workflowType == "" {
		workflowType = DefaultWorkflowType
	}

	input, err := buildInput(workflowType, tpl, detail)
	if err != nil {
		return nil, err
	}

	// The task id doubles as the run id: it is already a single NATS subject
	// token, so the run's lifecycle events land on mp.v1.run.<task_id>.event
	// where the mp.v1.run.*.event consumers can see them, and it makes the
	// task ⇄ run correlation readable with no extra mapping table.
	return &mpv1.StartWorkflowRequest{
		WorkflowType: workflowType,
		OrgId:        detail.orgID,
		RunId:        task.ID,
		Input:        input,
	}, nil
}

// buildInput assembles the workflow input.
//
// An explicit `workflow_input` in the template wins verbatim — the caller knows
// the target workflow's contract. Otherwise the task's own title/description
// become the run goal, which is the only sensible reading of a cron template
// that says "do this thing".
func buildInput(
	workflowType string,
	tpl taskTemplate,
	detail taskDetail,
) (*structpb.Struct, error) {
	if len(tpl.WorkflowInput) > 0 {
		var m map[string]any
		if err := json.Unmarshal(tpl.WorkflowInput, &m); err != nil {
			return nil, fmt.Errorf("taskexec: workflow_input is not a JSON object: %w", err)
		}
		s, err := structpb.NewStruct(m)
		if err != nil {
			return nil, fmt.Errorf("taskexec: workflow_input is not representable: %w", err)
		}
		return s, nil
	}

	goal := taskGoal(detail)
	if goal == "" {
		return nil, fmt.Errorf(
			"taskexec: task has neither a title/description to use as a goal nor an explicit workflow_input for %s",
			workflowType)
	}
	fields := map[string]any{"goal": goal}
	if policy := strings.TrimSpace(tpl.Policy); policy != "" {
		fields["policy"] = policy
	}
	s, err := structpb.NewStruct(fields)
	if err != nil {
		return nil, fmt.Errorf("taskexec: build workflow input: %w", err)
	}
	return s, nil
}

// taskGoal renders the task's human intent as a run goal.
func taskGoal(detail taskDetail) string {
	title := strings.TrimSpace(detail.title)
	description := strings.TrimSpace(detail.description)
	switch {
	case title != "" && description != "":
		return title + "\n\n" + description
	case title != "":
		return title
	default:
		return description
	}
}

// recordStarted appends the durable audit row linking task to workflow.
func (d *WorkflowDispatcher) recordStarted(
	ctx context.Context,
	task TaskRef,
	workflowType string,
	resp *mpv1.StartWorkflowResponse,
) error {
	payload, err := json.Marshal(map[string]string{
		"workflow_type":   workflowType,
		"workflow_id":     resp.GetWorkflowId(),
		"temporal_run_id": resp.GetTemporalRunId(),
		"run_id":          task.ID,
	})
	if err != nil {
		return err
	}
	_, err = d.pool.Exec(ctx, `
		INSERT INTO task_events (id, task_id, event_type, actor, payload, ts)
		VALUES ($1, $2, 'started', $3, $4, $5)
	`, "taskevt_"+uuid.NewString(), task.ID, dispatchActor, payload, time.Now().UTC())
	return err
}
