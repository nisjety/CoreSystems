package sessionreview

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"strconv"
	"strings"
	"time"

	"github.com/nats-io/nats.go"
	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/envelope"
	"github.com/triodelab/model-plane/services/capability-core/internal/learning"
	"github.com/triodelab/model-plane/services/capability-core/internal/llmreviewer"
	"github.com/triodelab/model-plane/services/capability-core/internal/skillsink"
	"github.com/triodelab/model-plane/services/capability-core/internal/telemetry"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// RunEventsStream is the JetStream stream RunConsumer's durable is bound
// to. Must match nats-provisioner's stream name exactly (services/
// nats-provisioner/main.go's streamConfigs, "MODEL_PLANE_RUN_EVENTS") --
// the same stream internal/runwatch.Notifier binds a separate durable to.
const RunEventsStream = "MODEL_PLANE_RUN_EVENTS"

// SkillReviewDurable is this consumer's durable name. Must exactly match
// the Durable value nats-provisioner registers server-side for this stream
// (see its consumerBindings) -- binding is Bind-only (never
// self-provisioning), so a name mismatch here fails RunConsumer's subscribe
// outright rather than silently missing events, the same contract
// runwatch.RunWatchDurable already relies on.
const SkillReviewDurable = "capability-core-skill-review"

// RunCompletedSubject is the NATS subject filter for run lifecycle events;
// RUN_COMPLETED envelopes arrive here and ParseRunCompleted selects them.
const RunCompletedSubject = "mp.v1.run.*.event"

// reviewSessionClient is the session-core surface the learning review needs:
// the transcript reader (ListConversation + ListAgentSkills, via the embedded
// sessionCoreReader) plus the skill writer (UpsertAgentSkill, for the sink).
// The generated *mpv1.SessionCoreClient satisfies it; tests inject a fake.
type reviewSessionClient interface {
	sessionCoreReader
	UpsertAgentSkill(ctx context.Context, in *mpv1.UpsertAgentSkillRequest, opts ...grpc.CallOption) (*mpv1.UpsertAgentSkillResponse, error)
}

// reviewInferenceClient is the inference-core surface the reviewer needs.
type reviewInferenceClient interface {
	Infer(ctx context.Context, in *mpv1.InferRequest, opts ...grpc.CallOption) (*mpv1.InferResponse, error)
}

// memoryControlsReader is the MemoryService question the review asks before it
// reads a conversation: may this user's runs be learned from? The generated
// *mpv1.MemoryServiceClient satisfies it; tests inject a fake.
type memoryControlsReader interface {
	GetMemoryControls(ctx context.Context, in *mpv1.GetMemoryControlsRequest, opts ...grpc.CallOption) (*mpv1.GetMemoryControlsResponse, error)
}

// Option configures a review consumer.
type Option func(*reviewOptions)

type reviewOptions struct {
	controls memoryControlsReader
	cost     *CostControls
}

// WithMemoryControls makes the review ask Session Core whether the run's owner
// allows learning before it reads their conversation. Without it the review
// relies on Session Core's own gates (the run is not announced, and the skill
// write is refused) and still pays for a review that write will refuse.
func WithMemoryControls(client memoryControlsReader) Option {
	return func(o *reviewOptions) { o.controls = client }
}

// WithCostControls bounds what each review may spend; see [CostControls]. Without
// it a review runs exactly as it did before the controls existed.
func WithCostControls(cost CostControls) Option {
	return func(o *reviewOptions) { o.cost = &cost }
}

// CostControls bound what one learning review may spend. A review is a model
// call over a whole conversation, so it is skipped before that call when the
// conversation is too short or too long to be worth reading, or when the owner
// or the organization has used up the day's reviews.
type CostControls struct {
	// MinUserTurns is the fewest user turns a conversation needs to be reviewed.
	MinUserTurns int
	// MaxTranscriptChars is the longest rendered transcript a review sends.
	MaxTranscriptChars int
	// MaxPerUserPerDay and MaxPerOrgPerDay cap the reviews started per UTC day.
	MaxPerUserPerDay int
	MaxPerOrgPerDay  int
	// Budget counts the day's reviews. Nil means no counter store is wired, and
	// every review is skipped: a cap that cannot be counted is not enforced.
	Budget ReviewBudget
}

// ReviewBudget counts reviews by key. Incr adds one to key and returns the count
// including it, and the key expires after ttl. Implementations must increment
// atomically, so two replicas never both see a count under the cap.
type ReviewBudget interface {
	Incr(ctx context.Context, key string, ttl time.Duration) (int64, error)
}

// HandleRunCompleted decodes a run-lifecycle envelope and, if it is a
// RUN_COMPLETED, runs one learning review end-to-end: fetch the transcript +
// existing skills (session-core), review (inference-core), and persist the
// survivors (session-core, provenance-guarded). Returns the number persisted
// (0 for any non-trigger event). This is the WHOLE trigger path from bytes to
// persistence and is unit-tested with fake clients.
//
// Every run that is not reviewed is logged as "learning review skipped" with
// a reason, apart from "learning review completed", which only a review that
// reached the model logs, with its candidate/selected/persisted counts.
//
// # Zero Data Retention gate
//
// A review turns conversation content into a durable skill, so this function is
// a content-persisting boundary and must not act on a no-retention run. The
// posture is checked BEFORE the transcript source is built, so a ZDR (or
// unattested) run never causes even a READ of the conversation — reading it and
// discarding the result later would already have moved the content across the
// boundary. Anything other than an explicit `zdr: false` is skipped; see
// [RetentionPosture] for why absence fails closed and what that costs.
func HandleRunCompleted(
	ctx context.Context,
	data []byte,
	sc reviewSessionClient,
	ic reviewInferenceClient,
	model string,
	opts ...Option,
) (int, error) {
	outcome, err := handleRunCompleted(ctx, data, sc, ic, model, opts...)
	return outcome.Persisted, err
}

// Skip reasons decided here rather than by the transcript source.
const (
	skipNotRunCompleted  learning.SkipReason = "not_run_completed"
	skipRetentionPosture learning.SkipReason = "retention_posture"
	skipProviderRefusal  learning.SkipReason = "provider_refusal"
	// The owner's own memory state: paused or off.
	skipMemoryState learning.SkipReason = "memory_state"
	// The organization's policy: memory disabled, the learning review not
	// opted in, or the policy unreadable.
	skipOrgPolicy learning.SkipReason = "org_policy"
	// The envelope names no owner, so a proposal would belong to no one.
	skipOwnerUnknown learning.SkipReason = "owner_unknown"
	// The cost controls: the conversation is too short or too long to review, or
	// the day's caps are used up, or the caps cannot be counted.
	skipTooFewTurns        learning.SkipReason = "too_few_turns"
	skipTranscriptTooLarge learning.SkipReason = "transcript_too_large"
	skipUserDailyCap       learning.SkipReason = "user_daily_cap"
	skipOrgDailyCap        learning.SkipReason = "org_daily_cap"
	skipBudgetUnavailable  learning.SkipReason = "budget_unavailable"
)

// reviewSkipped counts the reviews that did not reach the model, by skip reason.
// The reason is a closed token; the run, the owner and the content never appear.
var reviewSkipped, _ = otel.Meter(telemetry.MeterName).Int64Counter(
	"learning_review_skipped_total",
	metric.WithDescription("Learning reviews that did not reach the model, labelled by skip reason."),
)

// The cost controls' defaults, used when their switch is on and a bound is unset.
const (
	defaultMinUserTurns       = 3
	defaultMaxTranscriptChars = 60000
	defaultMaxPerUserPerDay   = 5
	defaultMaxPerOrgPerDay    = 100
	// reviewBudgetTTL outlives the UTC day its counts belong to, so no count
	// expires while that day is still current.
	reviewBudgetTTL = 48 * time.Hour
)

// Environment names of the cost controls. The switch reads only the literal
// "true", and the bounds are read only when the switch is on.
const (
	learningReviewCostEnv     = "LEARNING_REVIEW_COST_CONTROLS"
	learningReviewMinTurnsEnv = "LEARNING_REVIEW_MIN_USER_TURNS"
	learningReviewMaxCharsEnv = "LEARNING_REVIEW_MAX_TRANSCRIPT_CHARS"
	learningReviewUserCapEnv  = "LEARNING_REVIEW_MAX_PER_USER_PER_DAY"
	learningReviewOrgCapEnv   = "LEARNING_REVIEW_MAX_PER_ORG_PER_DAY"
)

// logSkip records a review that did not reach the model: counted by its reason,
// and logged against the run it concerns.
func logSkip(ctx context.Context, runID string, reason learning.SkipReason, attrs ...any) {
	reviewSkipped.Add(ctx, 1, metric.WithAttributes(attribute.String("reason", string(reason))))
	slog.Info("learning review skipped", append([]any{"run_id", runID, "reason", reason}, attrs...)...)
}

// learningSkip asks Session Core whether the run's owner allows learning, and
// returns why not as a skip reason ("" when it does). An answer that cannot be
// had is an error, not a yes: the delivery is retried.
func learningSkip(ctx context.Context, controls memoryControlsReader, ref SessionRef) (learning.SkipReason, error) {
	resp, err := controls.GetMemoryControls(ctx, &mpv1.GetMemoryControlsRequest{OrgId: ref.OrgID, UserId: ref.UserID})
	if err != nil {
		return "", fmt.Errorf("sessionreview: memory controls for run %s: %w", ref.RunID, err)
	}
	decision := resp.GetControls()
	if decision == nil {
		return "", fmt.Errorf("sessionreview: memory controls for run %s: empty response", ref.RunID)
	}
	if decision.GetLearningAllowed() {
		return "", nil
	}
	switch decision.GetReason() {
	case "user_paused", "user_off", "thread_off":
		return skipMemoryState, nil
	case "zdr":
		return skipRetentionPosture, nil
	default:
		// org_disabled, policy_unavailable, and "on" with learning refused: the
		// organization has not opted in. A reason this build does not know is
		// refused as the organization's, the stricter of the two.
		return skipOrgPolicy, nil
	}
}

// handleRunCompleted is [HandleRunCompleted] returning the full outcome.
func handleRunCompleted(
	ctx context.Context,
	data []byte,
	sc reviewSessionClient,
	ic reviewInferenceClient,
	model string,
	opts ...Option,
) (learning.ReviewOutcome, error) {
	var options reviewOptions
	for _, opt := range opts {
		opt(&options)
	}
	var env envelope.Envelope
	if err := json.Unmarshal(data, &env); err != nil {
		return learning.ReviewOutcome{}, fmt.Errorf("sessionreview: decode envelope: %w", err)
	}
	ref, ok := ParseRunCompleted(&env)
	if !ok {
		// Not a run-completion we act on. Not logged: every other lifecycle
		// event on the stream lands here.
		return learning.ReviewOutcome{Skipped: skipNotRunCompleted}, nil
	}
	// Retention gate — ahead of every session-core read, by construction.
	if posture := parseRetentionPosture(data); !posture.AllowsDerivedPersistence() {
		logSkip(ctx, ref.RunID, skipRetentionPosture, "posture", posture.String())
		return learning.ReviewOutcome{Skipped: skipRetentionPosture}, nil
	}
	// Owner gate, also ahead of every read. A proposal belongs to the person
	// whose conversation it came from, so a run with no owner is not reviewed,
	// and neither is one whose owner (or organization) does not allow learning.
	if ref.UserID == "" {
		logSkip(ctx, ref.RunID, skipOwnerUnknown)
		return learning.ReviewOutcome{Skipped: skipOwnerUnknown}, nil
	}
	if options.controls != nil {
		skip, err := learningSkip(ctx, options.controls, ref)
		if err != nil {
			return learning.ReviewOutcome{}, err
		}
		if skip != "" {
			logSkip(ctx, ref.RunID, skip)
			return learning.ReviewOutcome{Skipped: skip}, nil
		}
	}
	reviewer := llmreviewer.NewReviewer(ic, model, ref.OrgID)
	sink := skillsink.NewSessionCoreSink(sc, ref.OrgID, ref.UserID, ref.RunID, ref.ThreadID)
	var src TranscriptSource = NewSessionCoreTranscriptSource(sc)
	if options.cost != nil {
		src = costGatedSource{inner: src, cost: *options.cost, now: time.Now}
	}
	outcome, err := OnRunCompleted(ctx, ref, src, reviewer, sink)
	var responseError *llmreviewer.ResponseError
	if errors.As(err, &responseError) && responseError.Kind == "refused" {
		logSkip(ctx, ref.RunID, skipProviderRefusal)
		return learning.ReviewOutcome{Skipped: skipProviderRefusal}, nil
	}
	switch {
	case err != nil:
		// Logged, with its delivery count, by whoever settles the message.
	case outcome.Skipped != "":
		logSkip(ctx, ref.RunID, outcome.Skipped)
	default:
		slog.Info("learning review completed", "run_id", ref.RunID,
			"candidates", outcome.Candidates, "selected", outcome.Selected,
			"skills_persisted", outcome.Persisted)
	}
	return outcome, err
}

// ReviewRunCompleted is [HandleRunCompleted] returning the full outcome: why a
// run was not reviewed, or what its review did. The replay harness reads the
// outcome; the consumer settles on the error alone.
func ReviewRunCompleted(
	ctx context.Context,
	data []byte,
	sc reviewSessionClient,
	ic reviewInferenceClient,
	model string,
	opts ...Option,
) (learning.ReviewOutcome, error) {
	return handleRunCompleted(ctx, data, sc, ic, model, opts...)
}

// LearnRequest is one person's /learn (EP-11 S7): distil this thread into a
// skill proposal, with an optional note. Org and user come from the verified
// caller, and the caller has already checked that the user owns the thread.
type LearnRequest struct {
	OrgID    string
	UserID   string
	ThreadID string
	// Note is the person's own text, at most learning.MaxLearnNoteChars. It
	// reaches the reviewer as quoted data only.
	Note string
}

// Learner runs /learn through the learning review's own pipeline: the same
// memory gate, transcript source, caps, reviewer and proposal sink as a review of
// a completed run. A /learn is split in two so the person hears every refusal:
// [Learner.Prepare] runs every gate and reads the transcript, and the returned
// [LearnJob] makes the model call and writes the proposals afterwards.
type Learner struct {
	sc      reviewSessionClient
	ic      reviewInferenceClient
	model   string
	options reviewOptions
}

// NewLearner builds the /learn pipeline over the review's clients and options
// (normally the same ones the run consumer is given).
func NewLearner(sc reviewSessionClient, ic reviewInferenceClient, model string, opts ...Option) *Learner {
	learner := &Learner{sc: sc, ic: ic, model: model}
	for _, opt := range opts {
		opt(&learner.options)
	}
	return learner
}

// Skip reasons only a /learn can have.
const (
	// The request names no thread.
	skipThreadUnknown learning.SkipReason = "thread_unknown"
)

// learnCost is the cost controls a /learn runs under. A person asking is reason
// enough to read a short thread, so the minimum turn count does not apply, but
// the transcript bound and the daily caps do, shared with the background review.
// With the controls switched off a /learn still counts against the default caps
// and, with no counter store wired, is refused as budget_unavailable: a request a
// person can repeat at will must never run uncounted.
func (l *Learner) learnCost() CostControls {
	cost := CostControls{
		MaxTranscriptChars: defaultMaxTranscriptChars,
		MaxPerUserPerDay:   defaultMaxPerUserPerDay,
		MaxPerOrgPerDay:    defaultMaxPerOrgPerDay,
	}
	if l.options.cost != nil {
		cost = *l.options.cost
	}
	cost.MinUserTurns = 0
	return cost
}

// Prepare runs a /learn's gates in the background review's order and reads the
// transcript: the owner's memory state and the organization's review opt-in are
// asked before anything of the thread is read, then the transcript is read and
// held to the transcript bound and the daily caps. A skip reason says why the
// /learn is refused; nothing is read after a gate that refuses. An error means a
// gate could not be asked, which is never an allow.
func (l *Learner) Prepare(ctx context.Context, req LearnRequest) (*LearnJob, learning.SkipReason, error) {
	if req.OrgID == "" || req.UserID == "" {
		return nil, skipOwnerUnknown, nil
	}
	if req.ThreadID == "" {
		return nil, skipThreadUnknown, nil
	}
	ref := SessionRef{OrgID: req.OrgID, UserID: req.UserID, ThreadID: req.ThreadID}
	// The background review may run without the memory controls, relying on
	// Session Core not announcing the run. A /learn has no announcement in front
	// of it, so the question must be asked here or the /learn does not run.
	if l.options.controls == nil {
		return nil, "", errors.New("sessionreview: learn needs the memory controls")
	}
	skip, err := learningSkip(ctx, l.options.controls, ref)
	if err != nil {
		return nil, "", err
	}
	if skip != "" {
		return nil, skip, nil
	}
	src := costGatedSource{inner: NewSessionCoreTranscriptSource(l.sc), cost: l.learnCost(), now: time.Now}
	transcript, err := src.Fetch(ctx, ref)
	if err != nil {
		return nil, "", err
	}
	if transcript.Skip != "" {
		return nil, transcript.Skip, nil
	}
	return &LearnJob{learner: l, ref: ref, transcript: transcript, note: req.Note}, "", nil
}

// LearnJob is a /learn that cleared every gate: the transcript is read and the
// day's caps are counted. Run makes the one model call and writes the proposals.
type LearnJob struct {
	learner    *Learner
	ref        SessionRef
	transcript Transcript
	note       string
}

// Run reviews the transcript with the /learn prompt and writes what survives
// the retention policy as proposals through UpsertAgentSkill, origin
// background_review: disabled, owned by the person, awaiting their decision,
// each reason marked as theirs ([learning.LearnReasonPrefix]). Counts only are
// logged; the thread's content and the note never are.
func (j *LearnJob) Run(ctx context.Context) (learning.ReviewOutcome, error) {
	reviewer := learnReviewer{inner: llmreviewer.NewReviewer(j.learner.ic, j.learner.model, j.ref.OrgID)}
	sink := learnSink{inner: skillsink.NewSessionCoreSink(j.learner.sc, j.ref.OrgID, j.ref.UserID, "", j.ref.ThreadID)}
	text := j.transcript.Text + learning.LearnRequestBlock(j.note)
	outcome, err := learning.RunReviewOutcome(ctx, text, j.transcript.Existing, reviewer, sink)
	var responseError *llmreviewer.ResponseError
	if errors.As(err, &responseError) && responseError.Kind == "refused" {
		slog.Info("learn review skipped", "thread_id", j.ref.ThreadID, "reason", skipProviderRefusal)
		return learning.ReviewOutcome{Skipped: skipProviderRefusal}, nil
	}
	if err == nil {
		slog.Info("learn review completed", "thread_id", j.ref.ThreadID,
			"candidates", outcome.Candidates, "selected", outcome.Selected,
			"skills_persisted", outcome.Persisted)
	}
	return outcome, err
}

// learnReviewer sends the /learn prompt whatever prompt the review loop names,
// so the loop itself stays the background review's.
type learnReviewer struct {
	inner learning.Reviewer
}

func (r learnReviewer) Review(ctx context.Context, transcript string, existing []learning.ExistingSkill, _ string) ([]learning.SkillCandidate, error) {
	return r.inner.Review(ctx, transcript, existing, learning.LearnReviewPrompt)
}

// learnSink marks every proposal's reason as requested by the person, then
// writes through the review's own sink, so each proposal guard applies unchanged.
type learnSink struct {
	inner learning.Sink
}

func (s learnSink) Persist(ctx context.Context, skills []learning.SkillCandidate) (int, error) {
	marked := make([]learning.SkillCandidate, len(skills))
	for i, skill := range skills {
		skill.Reason = learning.LearnReason(skill.Reason)
		marked[i] = skill
	}
	return s.inner.Persist(ctx, marked)
}

// costGatedSource reads a run's transcript through the cost controls. A review
// reaches the model only when the conversation has enough user turns to learn
// from, fits the transcript bound, and is inside the owner's and the
// organization's daily caps. Every check runs on the transcript already read and
// before the model is called, so a skipped review costs a read and no tokens.
type costGatedSource struct {
	inner TranscriptSource
	cost  CostControls
	now   func() time.Time
}

// Fetch implements [TranscriptSource].
func (s costGatedSource) Fetch(ctx context.Context, ref SessionRef) (Transcript, error) {
	transcript, err := s.inner.Fetch(ctx, ref)
	if err != nil || transcript.Skip != "" {
		return transcript, err
	}
	if len(transcript.Text) > s.cost.MaxTranscriptChars {
		return Transcript{Skip: skipTranscriptTooLarge}, nil
	}
	if countUserTurns(transcript.Text) < s.cost.MinUserTurns {
		return Transcript{Skip: skipTooFewTurns}, nil
	}
	if skip := s.reserve(ctx, ref); skip != "" {
		return Transcript{Skip: skip}, nil
	}
	return transcript, nil
}

// reserve counts this review against the owner's and the organization's caps for
// the UTC day, and reports the cap it breaks. A store that cannot count fails
// closed: a cap that is not counted is not enforced.
func (s costGatedSource) reserve(ctx context.Context, ref SessionRef) learning.SkipReason {
	if s.cost.Budget == nil {
		return skipBudgetUnavailable
	}
	day := s.now().UTC().Format("2006-01-02")
	owner, err := s.cost.Budget.Incr(ctx, "learning-review:user:"+ref.OrgID+":"+ref.UserID+":"+day, reviewBudgetTTL)
	if err != nil {
		return skipBudgetUnavailable
	}
	if owner > int64(s.cost.MaxPerUserPerDay) {
		return skipUserDailyCap
	}
	org, err := s.cost.Budget.Incr(ctx, "learning-review:org:"+ref.OrgID+":"+day, reviewBudgetTTL)
	if err != nil {
		return skipBudgetUnavailable
	}
	if org > int64(s.cost.MaxPerOrgPerDay) {
		return skipOrgDailyCap
	}
	return ""
}

// countUserTurns counts the user turns of a rendered transcript. RenderTranscript
// writes one "ROLE: content" block per turn, separated by a blank line, so a turn
// starts after one. A blank line inside a message can only add a turn, which lets
// a short conversation through to the caps but never holds back a long one.
func countUserTurns(transcript string) int {
	return strings.Count("\n\n"+transcript, "\n\nUSER: ")
}

// CostControlsFromLookup reads the cost controls through lookup (os.Getenv in
// the service). They are off unless LEARNING_REVIEW_COST_CONTROLS is exactly
// "true", and with them off a review runs as it did before they existed. When
// they are on, an unset bound takes its default, and a malformed bound is an
// error, so the caller does not start a consumer it cannot bound. The returned
// controls carry no [ReviewBudget]: the Redis counter is not wired, so until a
// store is installed every review is skipped as budget_unavailable.
func CostControlsFromLookup(lookup func(string) string) (CostControls, bool, error) {
	if lookup(learningReviewCostEnv) != "true" {
		return CostControls{}, false, nil
	}
	cost := CostControls{
		MinUserTurns:       defaultMinUserTurns,
		MaxTranscriptChars: defaultMaxTranscriptChars,
		MaxPerUserPerDay:   defaultMaxPerUserPerDay,
		MaxPerOrgPerDay:    defaultMaxPerOrgPerDay,
	}
	bounds := []struct {
		env   string
		value *int
	}{
		{learningReviewMinTurnsEnv, &cost.MinUserTurns},
		{learningReviewMaxCharsEnv, &cost.MaxTranscriptChars},
		{learningReviewUserCapEnv, &cost.MaxPerUserPerDay},
		{learningReviewOrgCapEnv, &cost.MaxPerOrgPerDay},
	}
	for _, bound := range bounds {
		raw := lookup(bound.env)
		if raw == "" {
			continue
		}
		n, err := strconv.Atoi(raw)
		if err != nil || n < 1 {
			return CostControls{}, false, fmt.Errorf("sessionreview: %s must be a positive integer", bound.env)
		}
		*bound.value = n
	}
	return cost, true, nil
}

// Delay transient/schema failures rather than redelivering at once; the
// delivery count they are bounded by is the consumer's MaxDeliver.
func reviewRetryDelay(delivery uint64) time.Duration {
	delays := []time.Duration{30 * time.Second, 2 * time.Minute, 10 * time.Minute, 30 * time.Minute}
	if delivery < 1 {
		delivery = 1
	}
	if delivery > uint64(len(delays)) {
		return delays[len(delays)-1]
	}
	return delays[delivery-1]
}

func permanentReviewFailure(err error) bool {
	switch status.Code(err) {
	case codes.InvalidArgument, codes.PermissionDenied, codes.Unauthenticated, codes.FailedPrecondition:
		return true
	default:
		return false
	}
}

// Fallbacks for when the bound consumer's own limits cannot be read. They
// equal what nats-provisioner provisions for SkillReviewDurable.
const (
	defaultReviewAckWait    = 120 * time.Second
	defaultReviewMaxDeliver = 5
)

// reviewAckMargin is how long before the consumer's AckWait a review is cut
// off, leaving time to settle the message before JetStream redelivers it.
const reviewAckMargin = 20 * time.Second

// reviewLimits are the delivery limits of the bound durable. RunConsumer reads
// them from the server at bind time, so nats-provisioner is their one source
// and this consumer cannot drift from it.
type reviewLimits struct {
	ackWait    time.Duration
	maxDeliver int // <= 0: unlimited
}

func defaultReviewLimits() reviewLimits {
	return reviewLimits{ackWait: defaultReviewAckWait, maxDeliver: defaultReviewMaxDeliver}
}

// limitsFromConsumer takes the limits from the consumer's server-side config,
// keeping the default for an ack wait the server did not report.
func limitsFromConsumer(info *nats.ConsumerInfo) reviewLimits {
	limits := defaultReviewLimits()
	if info == nil {
		return limits
	}
	if info.Config.AckWait > 0 {
		limits.ackWait = info.Config.AckWait
	}
	limits.maxDeliver = info.Config.MaxDeliver
	return limits
}

// deadline bounds one review below the ack wait (100 s for the provisioned
// 120 s). A review still running when the ack wait expires is redelivered and
// reviewed a second time alongside the first.
func (l reviewLimits) deadline() time.Duration {
	if d := l.ackWait - reviewAckMargin; d >= l.ackWait/2 {
		return d
	}
	return l.ackWait / 2
}

// exhausted reports whether this delivery was the consumer's last.
func (l reviewLimits) exhausted(delivery uint64) bool {
	return l.maxDeliver > 0 && delivery >= uint64(l.maxDeliver)
}

// reviewDelivery is the part of a JetStream message a review settles.
// *nats.Msg satisfies it; tests inject a recorder.
type reviewDelivery interface {
	Metadata() (*nats.MsgMetadata, error)
	Ack(opts ...nats.AckOpt) error
	NakWithDelay(delay time.Duration, opts ...nats.AckOpt) error
	Term(opts ...nats.AckOpt) error
	InProgress(opts ...nats.AckOpt) error
}

// settleReview runs one review under the per-message deadline and settles its
// delivery: Ack when it completed or was skipped; Term on a permanent
// (configuration/authorization) failure; otherwise, including a review that
// ran out of time, a delayed Nak until the consumer's MaxDeliver is reached.
func settleReview(
	ctx context.Context,
	msg reviewDelivery,
	subject string,
	limits reviewLimits,
	review func(context.Context) (learning.ReviewOutcome, error),
) {
	// The ack wait runs from delivery, and a pushed message can sit in the
	// client queue behind another review for most of it. Restarting the ack
	// wait here is what makes the deadline below fall inside it.
	if err := msg.InProgress(); err != nil {
		slog.Warn("sessionreview: in-progress ack failed", "subject", subject, "error", err)
	}
	reviewCtx, cancel := context.WithTimeout(ctx, limits.deadline())
	outcome, herr := review(reviewCtx)
	timedOut := errors.Is(reviewCtx.Err(), context.DeadlineExceeded)
	cancel()
	if herr != nil {
		metadata, metadataErr := msg.Metadata()
		var delivery, sequence uint64
		if metadataErr == nil {
			delivery = metadata.NumDelivered
			sequence = metadata.Sequence.Stream
		}
		if !timedOut && permanentReviewFailure(herr) {
			slog.Error("learning review permanently failed; retained event requires recovery", "stream", RunEventsStream, "stream_sequence", sequence, "subject", subject, "code", status.Code(herr).String())
			if err := msg.Term(); err != nil {
				slog.Warn("sessionreview: term failed", "error", err)
			}
			return
		}
		if limits.exhausted(delivery) {
			// Leave the original envelope in the stream, unacknowledged. Operators
			// can inspect/replay this exact sequence after fixing the cause, without
			// logging customer content or silently marking a failed review done.
			slog.Error("learning review exhausted; retained event requires recovery", "stream", RunEventsStream, "stream_sequence", sequence, "subject", subject, "delivery", delivery, "max_deliver", limits.maxDeliver, "timed_out", timedOut, "error", herr)
			return
		}
		delay := reviewRetryDelay(delivery)
		slog.Warn("learning review failed; delayed redelivery", "subject", subject, "stream_sequence", sequence, "delivery", delivery, "retry_after", delay, "timed_out", timedOut, "deadline", limits.deadline(), "error", herr)
		if nerr := msg.NakWithDelay(delay); nerr != nil {
			slog.Warn("sessionreview: nak failed", "subject", subject, "error", nerr)
		}
		return
	}
	if outcome.Persisted > 0 {
		slog.Info("learning review persisted skills", "count", outcome.Persisted)
	}
	if aerr := msg.Ack(); aerr != nil {
		slog.Warn("sessionreview: ack failed", "subject", subject, "error", aerr)
	}
}

// RunConsumer binds the durable JetStream consumer (SkillReviewDurable on
// RunEventsStream) with manual ack and runs a learning review for each
// RUN_COMPLETED. Best-effort and self-contained: per-message decode/skip
// outcomes are always acked; a genuine review failure is Nak'd so JetStream
// redelivers it (bounded by the consumer's MaxDeliver, set server-side by
// nats-provisioner). Each review is cut off below the consumer's AckWait (see
// [reviewLimits.deadline]); both limits are read from the consumer at bind
// time. Blocks until ctx is cancelled.
//
// # Why JetStream, not a plain subscribe
//
// This used to be a plain core-NATS nc.Subscribe -- fire-and-forget, at-most-
// once, with no persistence and no redelivery. That was recorded elsewhere
// (internal/runwatch's package doc) as an acceptable tradeoff because this
// consumer's side effects were characterized as "cheap local reads/writes" --
// which is not what HandleRunCompleted actually does: it reads a full
// conversation transcript from session-core, calls inference-core for a real
// LLM review, and writes a durable skill back to session-core. Losing a
// message silently here means a run's transcript is never reviewed and never
// contributes a skill -- exactly the failure this project verified live on
// 2026-09-17: session-core had durably published 88 RUN_COMPLETED learning
// events (every one JetStream-acked, none stuck) while capability-core's
// plain subscribe received zero of them, and agent_skills held zero rows
// despite 147 completed runs. A durable JetStream consumer, mirroring
// internal/runwatch.Notifier's already-proven pattern, closes that gap: a
// message published while this process is down or reconnecting is still
// waiting in the stream the next time it binds.
//
// js must already have SkillReviewDurable provisioned on RunEventsStream (see
// nats-provisioner) -- this call binds, it never self-creates the consumer.
//
// The subscription path is exercised against a real NATS+JetStream server by
// TestRunConsumerAgainstLiveNATS. The injected gRPC clients still need a
// deployed session-core/inference-core/LLM for a full customer proof; the
// decode→review→persist logic is unit-tested with fakes here.
func RunConsumer(
	ctx context.Context,
	js nats.JetStreamContext,
	sc reviewSessionClient,
	ic reviewInferenceClient,
	model string,
	opts ...Option,
) error {
	// Read before binding so the very first delivery is already settled
	// against the server's limits. The bind below performs this same lookup,
	// so it needs no permission the consumer did not already have.
	limits := defaultReviewLimits()
	if info, err := js.ConsumerInfo(RunEventsStream, SkillReviewDurable); err != nil {
		slog.Warn("learning-review consumer limits unreadable; using provisioned defaults",
			"stream", RunEventsStream, "durable", SkillReviewDurable, "error", err,
			"ack_wait", limits.ackWait, "max_deliver", limits.maxDeliver)
	} else {
		limits = limitsFromConsumer(info)
	}
	sub, err := js.QueueSubscribe(RunCompletedSubject, SkillReviewDurable, func(msg *nats.Msg) {
		settleReview(ctx, msg, msg.Subject, limits, func(reviewCtx context.Context) (learning.ReviewOutcome, error) {
			return handleRunCompleted(reviewCtx, msg.Data, sc, ic, model, opts...)
		})
	}, nats.Bind(RunEventsStream, SkillReviewDurable), nats.ManualAck())
	if err != nil {
		return fmt.Errorf("sessionreview: bind %s/%s: %w", RunEventsStream, SkillReviewDurable, err)
	}
	slog.Info("learning-review consumer bound", "stream", RunEventsStream, "durable", SkillReviewDurable, "subject", RunCompletedSubject,
		"ack_wait", limits.ackWait, "max_deliver", limits.maxDeliver, "review_deadline", limits.deadline())
	<-ctx.Done()
	_ = sub.Unsubscribe()
	return nil
}
