package evaloptimizer

import (
	"context"
	"reflect"
	"testing"
)

// driveSteppers reimplements the loop using ONLY the exported stepping
// primitives, mirroring what the durable Temporal workflow driver does (each
// Invoke below is an activity there). It exists to prove the primitives
// reconstruct RunLoop's behaviour exactly — the property workflow replay
// depends on for crash-resume.
func driveSteppers(t *testing.T, cfg Config, inv ModelInvoker) Outcome {
	t.Helper()
	cfg = cfg.WithDefaults()
	if err := cfg.Validate(); err != nil {
		t.Fatalf("config invalid: %v", err)
	}

	var (
		rounds []Attempt
		spent  int
		prev   *Attempt
	)
	stop := StopMaxRounds
	for round := 0; round < cfg.MaxRounds; round++ {
		if cfg.BudgetExceeded(spent) {
			stop = StopBudgetExhausted
			break
		}
		genRes, err := inv.Invoke(context.Background(), cfg.GeneratorRequest(prev))
		if err != nil {
			t.Fatalf("generator leg: %v", err)
		}
		spent += genRes.TotalTokens()

		judgeRes, err := inv.Invoke(context.Background(), cfg.JudgeRequest(genRes.Content))
		if err != nil {
			t.Fatalf("judge leg: %v", err)
		}
		spent += judgeRes.TotalTokens()

		attempt := BuildAttempt(round, genRes, judgeRes)
		rounds = append(rounds, attempt)
		if cfg.RoundPassed(attempt) {
			stop = StopPassed
			break
		}
		a := attempt
		prev = &a
	}
	return BuildOutcome(rounds, stop, spent)
}

// TestSteppersMatchRunLoop proves driver equivalence: for every terminal shape
// (pass-first, loop-then-pass, cap, budget), the stepper-driven loop and
// RunLoop produce byte-identical outcomes and issue byte-identical request
// sequences. This is the invariant that lets the durable workflow claim the
// exact semantics the table-driven RunLoop tests establish.
func TestSteppersMatchRunLoop(t *testing.T) {
	tests := []struct {
		name   string
		cfg    func(Config) Config
		rounds []round
	}{
		{
			name:   "passes on first round",
			rounds: []round{{answer: "a0", genTokens: 10, verdict: passVerdict(0.95, "great"), judgeTokens: 5}},
		},
		{
			name: "loops then passes",
			rounds: []round{
				{answer: "a0", genTokens: 10, verdict: failVerdict(0.4, "revise"), judgeTokens: 5},
				{answer: "a1", genTokens: 12, verdict: passVerdict(0.9, "good"), judgeTokens: 6},
			},
		},
		{
			name: "hits round cap",
			cfg:  func(c Config) Config { c.MaxRounds = 2; return c },
			rounds: []round{
				{answer: "a0", genTokens: 8, verdict: failVerdict(0.4, "weak"), judgeTokens: 4},
				{answer: "a1", genTokens: 8, verdict: failVerdict(0.7, "closer"), judgeTokens: 4},
			},
		},
		{
			name:   "budget exhaustion",
			cfg:    func(c Config) Config { c.MaxRounds = 5; c.TotalTokenBudget = 50; return c },
			rounds: []round{{answer: "a0", genTokens: 30, verdict: failVerdict(0.5, "again"), judgeTokens: 30}},
		},
		{
			name:   "unparseable verdict",
			cfg:    func(c Config) Config { c.MaxRounds = 1; return c },
			rounds: []round{{answer: "a0", genTokens: 5, verdict: "not json", judgeTokens: 5}},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			cfg := baseConfig()
			if tt.cfg != nil {
				cfg = tt.cfg(cfg)
			}

			loopInv := &scriptedInvoker{t: t, legs: script(tt.rounds...)}
			loopOut, err := RunLoop(context.Background(), cfg, loopInv)
			if err != nil {
				t.Fatalf("RunLoop: %v", err)
			}

			stepInv := &scriptedInvoker{t: t, legs: script(tt.rounds...)}
			stepOut := driveSteppers(t, cfg, stepInv)

			if !reflect.DeepEqual(loopOut, stepOut) {
				t.Errorf("outcomes diverge:\nRunLoop:  %+v\nsteppers: %+v", loopOut, stepOut)
			}
			if !reflect.DeepEqual(loopInv.calls, stepInv.calls) {
				t.Errorf("request sequences diverge:\nRunLoop:  %+v\nsteppers: %+v", loopInv.calls, stepInv.calls)
			}
		})
	}
}
