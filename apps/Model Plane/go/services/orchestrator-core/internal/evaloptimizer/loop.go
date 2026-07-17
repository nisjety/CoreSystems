// Package evaloptimizer implements the evaluator-optimizer (a.k.a.
// generator+judge / reflexion) orchestration pattern: a generator model
// produces an answer, a distinct judge model grades it against a written
// rubric, and the work loops generator→judge until the answer passes the
// rubric or a bounded turn/token cap is hit.
//
// The loop is pure: it depends only on a ModelInvoker abstraction and the
// standard library, so it is exercised end-to-end by table-driven tests with
// a scripted fake invoker. The Model Plane wires the real path by supplying an
// inference-core-backed ModelInvoker from a Temporal activity (see
// cmd/activities); the loop itself performs no I/O, holds no clock, and uses
// no randomness, which keeps it deterministic and reusable.
package evaloptimizer

import (
	"context"
	"errors"
	"fmt"
	"strings"
)

// Chat roles used when composing generator/judge prompts.
const (
	RoleSystem    = "system"
	RoleUser      = "user"
	RoleAssistant = "assistant"
)

// Call-site labels stamped on each InvokeRequest so the invoker and telemetry
// can distinguish the two invocations of a round.
const (
	CallGenerator = "generator"
	CallJudge     = "judge"
)

// Stop reasons recorded on an Outcome. Every terminal state is observable.
const (
	// StopPassed: the judge accepted an attempt against the rubric.
	StopPassed = "passed"
	// StopMaxRounds: the round cap was reached without a passing attempt.
	StopMaxRounds = "max_rounds"
	// StopBudgetExhausted: the cumulative token budget was reached before a
	// passing attempt, so the loop stopped cleanly and returned best-effort.
	StopBudgetExhausted = "budget_exhausted"
	// StopError: a model invocation failed and the run was aborted. Reported
	// alongside a non-nil error and the best-effort partial transcript.
	StopError = "error"
)

// Message is a single chat turn passed to a model.
type Message struct {
	Role    string
	Content string
}

// InvokeRequest is one provider-agnostic model call. The activity layer maps
// it onto inference-core's InferRequest; ZDR and OrgID are carried on every
// request so tenant scope and Zero Data Retention propagate to the provider
// boundary on both the generator and judge legs.
type InvokeRequest struct {
	// Call is CallGenerator or CallJudge — the leg of the round this request
	// serves. The judge is always a separate InvokeRequest from the generator.
	Call string

	Model                  string
	Messages               []Message
	MaxTokens              int
	Temperature            float32
	StructuredOutputSchema string

	ZDR   bool
	OrgID string
}

// InvokeResult is a model response plus token accounting used for budgeting.
type InvokeResult struct {
	Content      string
	InputTokens  int
	OutputTokens int
	ModelUsed    string
}

// TotalTokens is the input+output token count charged against the budget.
func (r InvokeResult) TotalTokens() int { return r.InputTokens + r.OutputTokens }

// ModelInvoker performs a single model call. Implementations MUST treat every
// call as an independent invocation — the judge leg is never folded into the
// generator's call.
type ModelInvoker interface {
	Invoke(ctx context.Context, req InvokeRequest) (InvokeResult, error)
}

// Verdict is the judge's structured grade of one generator attempt.
type Verdict struct {
	Passed   bool    `json:"passed"`
	Score    float64 `json:"score"`
	Feedback string  `json:"feedback"`
}

// Attempt captures one generator→judge round.
type Attempt struct {
	Round       int     `json:"round"`
	Answer      string  `json:"answer"`
	Verdict     Verdict `json:"verdict"`
	GenTokens   int     `json:"gen_tokens"`
	JudgeTokens int     `json:"judge_tokens"`
	// VerdictParseError is non-empty when the judge response could not be
	// parsed into a Verdict; the round is then treated as a fail-closed
	// non-pass (Verdict zero-valued) so a flaky judge never yields a false
	// pass and the loop can retry.
	VerdictParseError string `json:"verdict_parse_error,omitempty"`
}

// RoundTokens is the tokens charged by this round (generator + judge).
func (a Attempt) RoundTokens() int { return a.GenTokens + a.JudgeTokens }

// Outcome is the loop result: the best-scoring attempt plus the full round
// transcript and an observable terminal reason.
type Outcome struct {
	Passed      bool      `json:"passed"`
	Best        Attempt   `json:"best"`
	Rounds      []Attempt `json:"rounds"`
	StopReason  string    `json:"stop_reason"`
	TotalTokens int       `json:"total_tokens"`
}

// RoundsRun is the number of generator→judge rounds executed.
func (o Outcome) RoundsRun() int { return len(o.Rounds) }

// Config parameterises one evaluator-optimizer run.
type Config struct {
	// Generator: the model, its system/instruction prompt, and the task to
	// satisfy.
	GeneratorModel  string
	GeneratorSystem string
	Task            string

	// Judge: the model, its instruction prompt, and the written rubric it
	// grades the OUTCOME against (not a fixed path).
	JudgeModel  string
	JudgeSystem string
	Rubric      string

	// MaxRounds bounds the number of generator→judge rounds (turn cap).
	MaxRounds int
	// PassThreshold, when > 0, additionally requires Verdict.Score >=
	// PassThreshold for a round to count as passed (Verdict.Passed must also
	// be true). When 0, a round passes on Verdict.Passed alone.
	PassThreshold float64

	// PerRoundTokenBudget, when > 0, caps the output tokens requested from the
	// generator and judge on each leg. TotalTokenBudget, when > 0, caps the
	// cumulative input+output tokens across the whole run; it is the hard stop
	// checked before each round.
	PerRoundTokenBudget int
	TotalTokenBudget    int

	// Per-leg output-token requests. Defaulted when <= 0.
	GeneratorMaxTokens int
	JudgeMaxTokens     int
	// Temperature is applied to the generator. The judge is always invoked at
	// temperature 0 for stable, reproducible grading.
	Temperature float32

	// ZDR propagates Zero Data Retention to both legs. OrgID is the tenant
	// scope stamped on every request.
	ZDR   bool
	OrgID string
}

// Defaults applied by withDefaults when a field is unset.
const (
	defaultMaxRounds          = 3
	defaultGeneratorMaxTokens = 1024
	defaultJudgeMaxTokens     = 512
	defaultTemperature        = 0.7
)

func (c Config) withDefaults() Config {
	if c.MaxRounds <= 0 {
		c.MaxRounds = defaultMaxRounds
	}
	if c.GeneratorMaxTokens <= 0 {
		c.GeneratorMaxTokens = defaultGeneratorMaxTokens
	}
	if c.JudgeMaxTokens <= 0 {
		c.JudgeMaxTokens = defaultJudgeMaxTokens
	}
	if c.Temperature == 0 {
		c.Temperature = defaultTemperature
	}
	return c
}

// Validate reports configuration errors that make a run meaningless. Defaults
// are assumed already applied.
func (c Config) Validate() error {
	switch {
	case strings.TrimSpace(c.GeneratorModel) == "":
		return errors.New("evaloptimizer: generator model is required")
	case strings.TrimSpace(c.JudgeModel) == "":
		return errors.New("evaloptimizer: judge model is required")
	case strings.TrimSpace(c.Task) == "":
		return errors.New("evaloptimizer: task is required")
	case strings.TrimSpace(c.Rubric) == "":
		return errors.New("evaloptimizer: rubric is required")
	case c.PassThreshold < 0 || c.PassThreshold > 1:
		return fmt.Errorf("evaloptimizer: pass threshold %.3f outside [0,1]", c.PassThreshold)
	case c.MaxRounds <= 0:
		return errors.New("evaloptimizer: max rounds must be positive")
	}
	return nil
}

// RunLoop runs the evaluator-optimizer loop. Each round invokes the generator,
// then — as a distinct call — the judge, feeding the judge's feedback into the
// next generator round. It stops on the first passing attempt, when the round
// cap is reached, or when the cumulative token budget is exhausted, and always
// returns the best-scoring attempt seen plus the full transcript.
//
// A ModelInvoker error aborts the run and is returned to the caller; the
// partial Outcome (rounds so far, best-effort best) is returned alongside so
// callers can still observe progress.
func RunLoop(ctx context.Context, cfg Config, inv ModelInvoker) (Outcome, error) {
	if inv == nil {
		return Outcome{}, errors.New("evaloptimizer: model invoker is nil")
	}
	cfg = cfg.withDefaults()
	if err := cfg.Validate(); err != nil {
		return Outcome{}, err
	}

	var (
		rounds   []Attempt
		best     Attempt
		haveBest bool
		spent    int
		prev     *Attempt
	)

	for round := 0; round < cfg.MaxRounds; round++ {
		// Hard stop, checked before spending on a new round (mirrors
		// cost-core's >= budget gate).
		if budgetExceeded(spent, cfg.TotalTokenBudget) {
			return finalize(rounds, best, haveBest, StopBudgetExhausted, spent), nil
		}

		// --- Generator leg ---
		genRes, err := inv.Invoke(ctx, cfg.generatorRequest(prev))
		if err != nil {
			return finalize(rounds, best, haveBest, StopError, spent),
				fmt.Errorf("evaloptimizer: generator round %d: %w", round, err)
		}
		spent += genRes.TotalTokens()

		// --- Judge leg (distinct invocation, never self-grading) ---
		judgeRes, err := inv.Invoke(ctx, cfg.judgeRequest(genRes.Content))
		if err != nil {
			return finalize(rounds, best, haveBest, StopError, spent),
				fmt.Errorf("evaloptimizer: judge round %d: %w", round, err)
		}
		spent += judgeRes.TotalTokens()

		attempt := Attempt{
			Round:       round,
			Answer:      genRes.Content,
			GenTokens:   genRes.TotalTokens(),
			JudgeTokens: judgeRes.TotalTokens(),
		}
		if v, perr := ParseVerdict(judgeRes.Content); perr != nil {
			// Fail closed: an unparseable grade is never a pass.
			attempt.VerdictParseError = perr.Error()
		} else {
			attempt.Verdict = v
		}

		rounds = append(rounds, attempt)
		if !haveBest || attempt.Verdict.Score > best.Verdict.Score {
			best, haveBest = attempt, true
		}

		if roundPassed(cfg, attempt) {
			// The accepted attempt is the best result on a pass.
			return finalize(rounds, attempt, true, StopPassed, spent), nil
		}

		a := attempt // avoid aliasing the loop variable
		prev = &a
	}

	return finalize(rounds, best, haveBest, StopMaxRounds, spent), nil
}

// roundPassed reports whether an attempt satisfies the rubric. The judge must
// assert Passed, and when a threshold is configured the score must clear it.
func roundPassed(cfg Config, a Attempt) bool {
	if a.VerdictParseError != "" || !a.Verdict.Passed {
		return false
	}
	if cfg.PassThreshold > 0 && a.Verdict.Score < cfg.PassThreshold {
		return false
	}
	return true
}

// budgetExceeded mirrors cost-core CheckBudget semantics: a limit of 0 means
// unlimited; otherwise spend >= limit is exhausted.
func budgetExceeded(spent, limit int) bool { return limit > 0 && spent >= limit }

func finalize(rounds []Attempt, best Attempt, haveBest bool, reason string, spent int) Outcome {
	o := Outcome{
		Rounds:      rounds,
		StopReason:  reason,
		TotalTokens: spent,
		Passed:      reason == StopPassed,
	}
	if haveBest {
		o.Best = best
	}
	return o
}

// generatorRequest composes the generator call for a round. On the first round
// it presents the task; on later rounds it threads the prior answer and the
// judge's feedback so the generator revises rather than restarts.
func (c Config) generatorRequest(prev *Attempt) InvokeRequest {
	msgs := []Message{
		{Role: RoleSystem, Content: c.GeneratorSystem},
		{Role: RoleUser, Content: c.Task},
	}
	if prev != nil {
		feedback := strings.TrimSpace(prev.Verdict.Feedback)
		if feedback == "" {
			feedback = "The previous answer did not satisfy the rubric."
		}
		msgs = append(msgs,
			Message{Role: RoleAssistant, Content: prev.Answer},
			Message{Role: RoleUser, Content: "The evaluator did not accept that answer.\n\nEvaluator feedback:\n" + feedback + "\n\nRevise your answer to satisfy the rubric. Return only the improved answer."},
		)
	}
	return InvokeRequest{
		Call:        CallGenerator,
		Model:       c.GeneratorModel,
		Messages:    msgs,
		MaxTokens:   effectiveMaxTokens(c.GeneratorMaxTokens, c.PerRoundTokenBudget),
		Temperature: c.Temperature,
		ZDR:         c.ZDR,
		OrgID:       c.OrgID,
	}
}

// judgeRequest composes the judge call. The judge grades the OUTCOME against
// the written rubric and returns a structured verdict; it runs at temperature
// 0 for reproducible grading and requests the verdict schema.
func (c Config) judgeRequest(answer string) InvokeRequest {
	system := strings.TrimSpace(c.JudgeSystem)
	if system == "" {
		system = "You are a strict evaluator. Grade the candidate answer only against the rubric."
	}
	system += "\n\nRubric:\n" + strings.TrimSpace(c.Rubric) +
		"\n\nRespond ONLY with a JSON object of the form " +
		`{"passed": bool, "score": number in [0,1], "feedback": string}. ` +
		"Set passed=true only when the answer fully satisfies the rubric. " +
		"When passed=false, feedback MUST state concretely what to fix."

	user := "Task:\n" + strings.TrimSpace(c.Task) + "\n\nCandidate answer:\n" + answer

	return InvokeRequest{
		Call:                   CallJudge,
		Model:                  c.JudgeModel,
		Messages:               []Message{{Role: RoleSystem, Content: system}, {Role: RoleUser, Content: user}},
		MaxTokens:              effectiveMaxTokens(c.JudgeMaxTokens, c.PerRoundTokenBudget),
		Temperature:            0,
		StructuredOutputSchema: VerdictSchema,
		ZDR:                    c.ZDR,
		OrgID:                  c.OrgID,
	}
}

// effectiveMaxTokens caps a leg's requested output tokens by the per-round
// budget when one is set (enforcement in code, not hope). A configured value
// of 0 defers to the per-round budget; both 0 defers to the provider default.
func effectiveMaxTokens(configured, perRoundBudget int) int {
	switch {
	case perRoundBudget <= 0:
		return configured
	case configured <= 0:
		return perRoundBudget
	case perRoundBudget < configured:
		return perRoundBudget
	default:
		return configured
	}
}
