package evaloptimizer

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"testing"
)

// ── scripted fake invoker ────────────────────────────────────────────────────

// leg is one scripted model response: its content plus the tokens it "charges"
// and an optional error to return instead.
type leg struct {
	content string
	tokens  int
	err     error
}

// scriptedInvoker replays scripted legs in call order and records every
// request it received, so tests can assert on invocation count, call labels,
// feedback threading, ZDR propagation, and token caps.
type scriptedInvoker struct {
	t     *testing.T
	legs  []leg
	calls []InvokeRequest
}

func (s *scriptedInvoker) Invoke(_ context.Context, req InvokeRequest) (InvokeResult, error) {
	i := len(s.calls)
	s.calls = append(s.calls, req)
	if i >= len(s.legs) {
		s.t.Fatalf("unexpected invoke #%d (call=%s): script exhausted", i, req.Call)
		return InvokeResult{}, nil
	}
	l := s.legs[i]
	if l.err != nil {
		return InvokeResult{}, l.err
	}
	return InvokeResult{Content: l.content, OutputTokens: l.tokens}, nil
}

// round bundles a generator answer with the judge verdict that grades it.
type round struct {
	answer      string
	genTokens   int
	verdict     string
	judgeTokens int
}

// script flattens rounds into the gen,judge,gen,judge… leg order RunLoop uses.
func script(rounds ...round) []leg {
	legs := make([]leg, 0, len(rounds)*2)
	for _, r := range rounds {
		legs = append(legs,
			leg{content: r.answer, tokens: r.genTokens},
			leg{content: r.verdict, tokens: r.judgeTokens},
		)
	}
	return legs
}

func passVerdict(score float64, feedback string) string {
	return fmt.Sprintf(`{"passed":true,"score":%g,"feedback":%q}`, score, feedback)
}

func failVerdict(score float64, feedback string) string {
	return fmt.Sprintf(`{"passed":false,"score":%g,"feedback":%q}`, score, feedback)
}

// baseConfig is a valid config; individual tests override fields.
func baseConfig() Config {
	return Config{
		GeneratorModel:  "claude-sonnet-5",
		GeneratorSystem: "You are a helpful assistant.",
		Task:            "Write a haiku about durability.",
		JudgeModel:      "claude-haiku-4-5",
		JudgeSystem:     "You grade haiku.",
		Rubric:          "Must be 3 lines and mention durability.",
		MaxRounds:       3,
		OrgID:           "org-1",
	}
}

// ── the four mandated loop scenarios (+ threshold, parse-failure) ────────────

func TestRunLoop(t *testing.T) {
	tests := []struct {
		name           string
		cfg            func(Config) Config
		rounds         []round
		wantStop       string
		wantPassed     bool
		wantRoundsRun  int
		wantBestRound  int
		wantBestAnswer string
		wantTotalToks  int
	}{
		{
			name:           "passes on first round",
			rounds:         []round{{answer: "ans-0", genTokens: 10, verdict: passVerdict(0.95, "great"), judgeTokens: 5}},
			wantStop:       StopPassed,
			wantPassed:     true,
			wantRoundsRun:  1,
			wantBestRound:  0,
			wantBestAnswer: "ans-0",
			wantTotalToks:  15,
		},
		{
			name: "loops then passes",
			rounds: []round{
				{answer: "ans-0", genTokens: 10, verdict: failVerdict(0.4, "add a durability reference"), judgeTokens: 5},
				{answer: "ans-1", genTokens: 12, verdict: passVerdict(0.9, "now correct"), judgeTokens: 6},
			},
			wantStop:       StopPassed,
			wantPassed:     true,
			wantRoundsRun:  2,
			wantBestRound:  1,
			wantBestAnswer: "ans-1",
			wantTotalToks:  33,
		},
		{
			name:          "hits cap returns best effort with did-not-pass",
			cfg:           func(c Config) Config { c.MaxRounds = 2; return c },
			rounds:        []round{{answer: "ans-0", genTokens: 8, verdict: failVerdict(0.4, "weak"), judgeTokens: 4}, {answer: "ans-1", genTokens: 8, verdict: failVerdict(0.7, "closer"), judgeTokens: 4}},
			wantStop:      StopMaxRounds,
			wantPassed:    false,
			wantRoundsRun: 2,
			wantBestRound: 1, // 0.7 > 0.4 → higher-scoring attempt is best
			wantTotalToks: 24,
		},
		{
			name:          "budget exhaustion stops cleanly",
			cfg:           func(c Config) Config { c.MaxRounds = 5; c.TotalTokenBudget = 50; return c },
			rounds:        []round{{answer: "ans-0", genTokens: 30, verdict: failVerdict(0.5, "again"), judgeTokens: 30}},
			wantStop:      StopBudgetExhausted,
			wantPassed:    false,
			wantRoundsRun: 1, // round 1 never starts: spent(60) >= budget(50)
			wantBestRound: 0,
			wantTotalToks: 60,
		},
		{
			name:          "pass asserted but score below threshold does not pass",
			cfg:           func(c Config) Config { c.MaxRounds = 1; c.PassThreshold = 0.8; return c },
			rounds:        []round{{answer: "ans-0", genTokens: 5, verdict: passVerdict(0.5, "meh"), judgeTokens: 5}},
			wantStop:      StopMaxRounds,
			wantPassed:    false,
			wantRoundsRun: 1,
			wantBestRound: 0,
			wantTotalToks: 10,
		},
		{
			name:          "unparseable verdict fails closed",
			cfg:           func(c Config) Config { c.MaxRounds = 1; return c },
			rounds:        []round{{answer: "ans-0", genTokens: 5, verdict: "the answer looks fine to me", judgeTokens: 5}},
			wantStop:      StopMaxRounds,
			wantPassed:    false,
			wantRoundsRun: 1,
			wantBestRound: 0,
			wantTotalToks: 10,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			cfg := baseConfig()
			if tt.cfg != nil {
				cfg = tt.cfg(cfg)
			}
			inv := &scriptedInvoker{t: t, legs: script(tt.rounds...)}

			out, err := RunLoop(context.Background(), cfg, inv)
			if err != nil {
				t.Fatalf("RunLoop returned error: %v", err)
			}

			if out.StopReason != tt.wantStop {
				t.Errorf("StopReason = %q, want %q", out.StopReason, tt.wantStop)
			}
			if out.Passed != tt.wantPassed {
				t.Errorf("Passed = %v, want %v", out.Passed, tt.wantPassed)
			}
			if out.RoundsRun() != tt.wantRoundsRun {
				t.Errorf("RoundsRun = %d, want %d", out.RoundsRun(), tt.wantRoundsRun)
			}
			if out.Best.Round != tt.wantBestRound {
				t.Errorf("Best.Round = %d, want %d", out.Best.Round, tt.wantBestRound)
			}
			if tt.wantBestAnswer != "" && out.Best.Answer != tt.wantBestAnswer {
				t.Errorf("Best.Answer = %q, want %q", out.Best.Answer, tt.wantBestAnswer)
			}
			if out.TotalTokens != tt.wantTotalToks {
				t.Errorf("TotalTokens = %d, want %d", out.TotalTokens, tt.wantTotalToks)
			}
			// The invoker is called exactly twice per executed round: the
			// generator and the distinct judge.
			if len(inv.calls) != tt.wantRoundsRun*2 {
				t.Errorf("invoke count = %d, want %d (2 per round)", len(inv.calls), tt.wantRoundsRun*2)
			}
		})
	}
}

// ── judge is a distinct invocation, in the right order ───────────────────────

func TestRunLoop_JudgeIsDistinctInvocation(t *testing.T) {
	inv := &scriptedInvoker{t: t, legs: script(round{answer: "a", genTokens: 1, verdict: passVerdict(1, "ok"), judgeTokens: 1})}
	if _, err := RunLoop(context.Background(), baseConfig(), inv); err != nil {
		t.Fatal(err)
	}
	if len(inv.calls) != 2 {
		t.Fatalf("want 2 calls, got %d", len(inv.calls))
	}
	if inv.calls[0].Call != CallGenerator {
		t.Errorf("call[0].Call = %q, want %q", inv.calls[0].Call, CallGenerator)
	}
	if inv.calls[1].Call != CallJudge {
		t.Errorf("call[1].Call = %q, want %q", inv.calls[1].Call, CallJudge)
	}
	// The judge grades the generator's actual answer — a separate request that
	// carries the answer, not the generator's own call.
	if !strings.Contains(lastUser(inv.calls[1]), "a") {
		t.Errorf("judge request should contain the candidate answer")
	}
	if inv.calls[1].StructuredOutputSchema == "" {
		t.Errorf("judge request should request the structured verdict schema")
	}
}

// ── feedback is threaded into the next generator round ───────────────────────

func TestRunLoop_FeedbackThreadedToGenerator(t *testing.T) {
	const fb = "mention durability explicitly"
	inv := &scriptedInvoker{t: t, legs: script(
		round{answer: "first", genTokens: 5, verdict: failVerdict(0.3, fb), judgeTokens: 5},
		round{answer: "second", genTokens: 5, verdict: passVerdict(0.9, "good"), judgeTokens: 5},
	)}
	if _, err := RunLoop(context.Background(), baseConfig(), inv); err != nil {
		t.Fatal(err)
	}
	// calls: [gen0, judge0, gen1, judge1]
	gen1 := inv.calls[2]
	if gen1.Call != CallGenerator {
		t.Fatalf("call[2] should be the second generator round, got %q", gen1.Call)
	}
	joined := allContent(gen1)
	if !strings.Contains(joined, fb) {
		t.Errorf("second generator round missing judge feedback %q in %q", fb, joined)
	}
	if !strings.Contains(joined, "first") {
		t.Errorf("second generator round should include the prior answer for revision")
	}
}

// ── ZDR and OrgID propagate to every leg ─────────────────────────────────────

func TestRunLoop_ZDRPropagatesToEveryInvocation(t *testing.T) {
	cfg := baseConfig()
	cfg.ZDR = true
	cfg.OrgID = "org-zdr"
	inv := &scriptedInvoker{t: t, legs: script(
		round{answer: "a", genTokens: 5, verdict: failVerdict(0.2, "no"), judgeTokens: 5},
		round{answer: "b", genTokens: 5, verdict: passVerdict(0.9, "yes"), judgeTokens: 5},
	)}
	if _, err := RunLoop(context.Background(), cfg, inv); err != nil {
		t.Fatal(err)
	}
	if len(inv.calls) == 0 {
		t.Fatal("no invocations recorded")
	}
	for i, c := range inv.calls {
		if !c.ZDR {
			t.Errorf("call #%d (%s) did not carry ZDR — a ZDR run must never leak to a retaining provider", i, c.Call)
		}
		if c.OrgID != "org-zdr" {
			t.Errorf("call #%d (%s) OrgID = %q, want org-zdr", i, c.Call, c.OrgID)
		}
	}
}

// ── per-round token budget caps the requested output tokens on both legs ─────

func TestRunLoop_PerRoundTokenBudgetCapsRequests(t *testing.T) {
	cfg := baseConfig()
	cfg.MaxRounds = 1
	cfg.GeneratorMaxTokens = 4096
	cfg.JudgeMaxTokens = 4096
	cfg.PerRoundTokenBudget = 100
	inv := &scriptedInvoker{t: t, legs: script(round{answer: "a", genTokens: 1, verdict: passVerdict(1, "ok"), judgeTokens: 1})}
	if _, err := RunLoop(context.Background(), cfg, inv); err != nil {
		t.Fatal(err)
	}
	for i, c := range inv.calls {
		if c.MaxTokens != 100 {
			t.Errorf("call #%d (%s) MaxTokens = %d, want 100 (capped by per-round budget)", i, c.Call, c.MaxTokens)
		}
	}
}

// ── judge always runs at temperature 0 for stable grading ────────────────────

func TestRunLoop_JudgeTemperatureZero(t *testing.T) {
	cfg := baseConfig()
	cfg.MaxRounds = 1
	cfg.Temperature = 0.9
	inv := &scriptedInvoker{t: t, legs: script(round{answer: "a", genTokens: 1, verdict: passVerdict(1, "ok"), judgeTokens: 1})}
	if _, err := RunLoop(context.Background(), cfg, inv); err != nil {
		t.Fatal(err)
	}
	if got := inv.calls[0].Temperature; got != 0.9 {
		t.Errorf("generator Temperature = %v, want 0.9", got)
	}
	if got := inv.calls[1].Temperature; got != 0 {
		t.Errorf("judge Temperature = %v, want 0", got)
	}
}

// ── invoker errors abort with a partial outcome ──────────────────────────────

func TestRunLoop_InvokerErrors(t *testing.T) {
	sentinel := errors.New("provider down")

	t.Run("generator error", func(t *testing.T) {
		inv := &scriptedInvoker{t: t, legs: []leg{{err: sentinel}}}
		_, err := RunLoop(context.Background(), baseConfig(), inv)
		if !errors.Is(err, sentinel) {
			t.Fatalf("want sentinel error, got %v", err)
		}
	})

	t.Run("judge error after a generated answer", func(t *testing.T) {
		inv := &scriptedInvoker{t: t, legs: []leg{{content: "ans", tokens: 5}, {err: sentinel}}}
		out, err := RunLoop(context.Background(), baseConfig(), inv)
		if !errors.Is(err, sentinel) {
			t.Fatalf("want sentinel error, got %v", err)
		}
		if out.StopReason == StopPassed {
			t.Errorf("a judge failure must not report a pass")
		}
	})
}

// ── config validation ────────────────────────────────────────────────────────

func TestRunLoop_ValidationErrors(t *testing.T) {
	nop := &scriptedInvoker{t: t}
	cases := map[string]func(Config) Config{
		"missing generator model": func(c Config) Config { c.GeneratorModel = ""; return c },
		"missing judge model":     func(c Config) Config { c.JudgeModel = ""; return c },
		"missing task":            func(c Config) Config { c.Task = ""; return c },
		"missing rubric":          func(c Config) Config { c.Rubric = ""; return c },
		"threshold out of range":  func(c Config) Config { c.PassThreshold = 1.5; return c },
	}
	for name, mut := range cases {
		t.Run(name, func(t *testing.T) {
			if _, err := RunLoop(context.Background(), mut(baseConfig()), nop); err == nil {
				t.Fatal("expected a validation error, got nil")
			}
			if len(nop.calls) != 0 {
				t.Fatalf("invalid config must not invoke the model, saw %d calls", len(nop.calls))
			}
		})
	}
}

func TestRunLoop_NilInvoker(t *testing.T) {
	if _, err := RunLoop(context.Background(), baseConfig(), nil); err == nil {
		t.Fatal("expected an error for a nil invoker")
	}
}

func TestRunLoop_AppliesDefaults(t *testing.T) {
	cfg := Config{
		GeneratorModel: "g",
		JudgeModel:     "j",
		Task:           "t",
		Rubric:         "r",
		// MaxRounds left 0 → defaults to defaultMaxRounds; all rounds fail.
	}
	rounds := make([]round, defaultMaxRounds)
	for i := range rounds {
		rounds[i] = round{answer: fmt.Sprintf("a%d", i), genTokens: 1, verdict: failVerdict(0.1, "no"), judgeTokens: 1}
	}
	inv := &scriptedInvoker{t: t, legs: script(rounds...)}
	out, err := RunLoop(context.Background(), cfg, inv)
	if err != nil {
		t.Fatal(err)
	}
	if out.RoundsRun() != defaultMaxRounds {
		t.Errorf("RoundsRun = %d, want default %d", out.RoundsRun(), defaultMaxRounds)
	}
}

// ── helpers ──────────────────────────────────────────────────────────────────

func lastUser(req InvokeRequest) string {
	for i := len(req.Messages) - 1; i >= 0; i-- {
		if req.Messages[i].Role == RoleUser {
			return req.Messages[i].Content
		}
	}
	return ""
}

func allContent(req InvokeRequest) string {
	var b strings.Builder
	for _, m := range req.Messages {
		b.WriteString(m.Content)
		b.WriteByte('\n')
	}
	return b.String()
}
