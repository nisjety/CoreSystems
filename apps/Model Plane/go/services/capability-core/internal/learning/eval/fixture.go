// Package eval replays the seeded learning-review cases through the production
// review path, sessionreview.ReviewRunCompleted, with fakes at its two external
// boundaries: the session-core and inference-core clients. A case is decided by
// the gates a live run passes, in the same order. The liveeval build tag runs the
// cases that reach a real model under a spend budget (see live_test.go).
//
// No case holds a real conversation: every transcript is written for its case.
package eval

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
)

// SeededDir is the directory of seeded cases, relative to this package.
const SeededDir = "testdata/seeded"

// caseID is the identifier shape the evidence contract accepts for a case.
var caseID = regexp.MustCompile(`^[a-z][a-z0-9-]{2,127}$`)

// Fixture is one seeded case.
type Fixture struct {
	ID          string `json:"id"`
	Category    string `json:"category"`
	Description string `json:"description"`
	// Upstream names the gate that decides the case before capability-core sees
	// the run. An upstream case has no attempts; that gate's own tests cover it.
	Upstream string `json:"upstream,omitempty"`
	// Gap names a control this build does not have yet. The case pins what the
	// build does today, so the gap is reported rather than passed silently.
	Gap string `json:"gap,omitempty"`
	// Scripted marks a case whose subject is the model's own answer (a refusal, a
	// malformed reply). Only the deterministic replay runs it; live mode does not.
	Scripted bool `json:"scripted,omitempty"`
	// Cost turns the cost controls on for the case; nil leaves them off.
	Cost *CostFixture `json:"cost,omitempty"`
	// Controls answers the owner's and the organization's memory-controls question.
	Controls Controls `json:"controls"`
	// Conversation is the thread the run belongs to.
	Conversation []Turn `json:"conversation"`
	// ExistingSkills are the skills the owner can already see.
	ExistingSkills []Skill `json:"existing_skills"`
	// ModelReply and ModelStopReason are the model's answer to the review.
	ModelReply      string `json:"model_reply"`
	ModelStopReason string `json:"model_stop_reason"`
	// Run is the envelope every attempt starts from.
	Run Run `json:"run"`
	// Attempts are the runs replayed in order against one shared budget.
	Attempts []Attempt `json:"attempts"`
	Expect   Expect    `json:"expect"`
	// Live bounds the proposals a live model may produce for the case. A case that
	// reaches the model and is not scripted must set it.
	Live *Bounds `json:"live,omitempty"`

	digest string
}

// CostFixture is the cost controls of one case.
type CostFixture struct {
	MinUserTurns       int `json:"min_user_turns"`
	MaxTranscriptChars int `json:"max_transcript_chars"`
	MaxPerUserPerDay   int `json:"max_per_user_per_day"`
	MaxPerOrgPerDay    int `json:"max_per_org_per_day"`
	// Budget is "memory" (an in-memory counter), "absent" (no counter wired) or
	// "failing" (a counter that errors on every increment).
	Budget string `json:"budget"`
}

// Controls is the memory-controls answer a case replays.
type Controls struct {
	LearningAllowed bool   `json:"learning_allowed"`
	Reason          string `json:"reason"`
}

// Turn is one message of the conversation. SourceScope "conversation" marks a
// private thread, which the review never learns from.
type Turn struct {
	Role        string `json:"role"`
	Content     string `json:"content"`
	SourceScope string `json:"source_scope,omitempty"`
}

// Skill is a skill the owner can already see.
type Skill struct {
	Name   string `json:"name"`
	Origin string `json:"origin"`
}

// Run is the RUN_COMPLETED envelope a case starts from.
type Run struct {
	EventType string `json:"event_type"`
	OrgID     string `json:"org_id"`
	UserID    string `json:"user_id"`
	RunID     string `json:"run_id"`
	ThreadID  string `json:"thread_id"`
	// ZDR is the envelope's zero-data-retention flag: nil when absent, which fails
	// closed, otherwise the declared value.
	ZDR *bool `json:"zdr"`
}

// Attempt is one replayed run. UserID, when set, is the owner of that run.
type Attempt struct {
	UserID string `json:"user_id,omitempty"`
}

// Expect is what a replayed case must do.
type Expect struct {
	// Outcomes holds, for each attempt in order, the skip reason, or "" for a
	// review that ran, "error" for one that failed and will be redelivered, or
	// "limit_reached" for one the live spend budget stopped.
	Outcomes        []string `json:"outcomes"`
	ModelCalls      int      `json:"model_calls"`
	TranscriptReads int      `json:"transcript_reads"`
	ProposalCount   int      `json:"proposal_count"`
	// Proposal, when set, is the shape every proposal must have.
	Proposal *Proposal `json:"proposal,omitempty"`
}

// Proposal is the shape a learned skill must have when it reaches the store.
type Proposal struct {
	Scope       string `json:"scope"`
	Enabled     bool   `json:"enabled"`
	ReviewState string `json:"review_state"`
	Origin      string `json:"origin"`
}

// Bounds are the inclusive proposal counts a live model may produce for a case.
type Bounds struct {
	Min int `json:"min"`
	Max int `json:"max"`
}

// Load reads every seeded case in dir, in file-name order, and checks each one's
// shape. A fixture is decoded strictly, so a misspelt key fails the load rather
// than being ignored.
func Load(dir string) ([]Fixture, error) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return nil, fmt.Errorf("eval: read %s: %w", dir, err)
	}
	var names []string
	for _, e := range entries {
		if !e.IsDir() && filepath.Ext(e.Name()) == ".json" {
			names = append(names, e.Name())
		}
	}
	sort.Strings(names)
	seen := map[string]bool{}
	var out []Fixture
	for _, name := range names {
		raw, err := os.ReadFile(filepath.Join(dir, name))
		if err != nil {
			return nil, fmt.Errorf("eval: read %s: %w", name, err)
		}
		var f Fixture
		dec := json.NewDecoder(bytes.NewReader(raw))
		dec.DisallowUnknownFields()
		if err := dec.Decode(&f); err != nil {
			return nil, fmt.Errorf("eval: %s: %w", name, err)
		}
		if err := validate(f); err != nil {
			return nil, fmt.Errorf("eval: %s: %w", name, err)
		}
		if seen[f.ID] {
			return nil, fmt.Errorf("eval: %s: duplicate case id %q", name, f.ID)
		}
		seen[f.ID] = true
		sum := sha256.Sum256(raw)
		f.digest = hex.EncodeToString(sum[:])
		out = append(out, f)
	}
	if len(out) == 0 {
		return nil, fmt.Errorf("eval: no seeded cases in %s", dir)
	}
	return out, nil
}

// validate rejects a case that could not say anything: an id the evidence contract
// refuses, no category, an upstream case that also replays attempts, outcomes that do
// not line up with attempts, or a model-reaching case with no bounds for live runs.
func validate(f Fixture) error {
	if !caseID.MatchString(f.ID) {
		return fmt.Errorf("case id %q must be lowercase kebab-case", f.ID)
	}
	if f.Category == "" {
		return fmt.Errorf("case %s has no category", f.ID)
	}
	if f.Upstream != "" {
		if len(f.Attempts) != 0 || len(f.Expect.Outcomes) != 0 {
			return fmt.Errorf("case %s is upstream and must not replay attempts", f.ID)
		}
		return nil
	}
	if len(f.Attempts) == 0 {
		return fmt.Errorf("case %s has no attempts", f.ID)
	}
	if len(f.Expect.Outcomes) != len(f.Attempts) {
		return fmt.Errorf("case %s has %d attempts but %d expected outcomes", f.ID, len(f.Attempts), len(f.Expect.Outcomes))
	}
	if f.Expect.ModelCalls > 0 && !f.Scripted && f.Live == nil {
		return fmt.Errorf("case %s reaches the model and needs live bounds", f.ID)
	}
	if f.Live != nil && (f.Live.Min < 0 || f.Live.Max < f.Live.Min) {
		return fmt.Errorf("case %s has invalid live bounds %+v", f.ID, *f.Live)
	}
	if f.Cost != nil {
		switch f.Cost.Budget {
		case "memory", "absent", "failing":
		default:
			return fmt.Errorf("case %s has unknown budget %q", f.ID, f.Cost.Budget)
		}
	}
	return nil
}
