package workspace

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"
)

// Everything the store refuses before it touches the database is checked without one.
func TestTheStoreRefusesWhatItWillNotKeep(t *testing.T) {
	s := New(nil)
	ctx := context.Background()
	for name, fn := range map[string]func() error{
		"a profile that is not an object": func() error {
			_, err := s.PutProfile(ctx, "org-a", "user-1", json.RawMessage(`[1,2]`))
			return err
		},
		"a profile over the size limit": func() error {
			big := `{"keywords":["` + strings.Repeat("x", maxProfileBytes) + `"]}`
			_, err := s.PutProfile(ctx, "org-a", "user-1", json.RawMessage(big))
			return err
		},
		"a stage that is not one of the pipeline's": func() error {
			_, err := s.SetStage(ctx, "org-a", "user-1", "2026-116131", "archived")
			return err
		},
		"a notice id of no known shape": func() error {
			_, err := s.SetStage(ctx, "org-a", "user-1", "../../etc", "inbox")
			return err
		},
		"an organisation id with a space": func() error {
			_, err := s.PutProfile(ctx, "org a", "user-1", json.RawMessage(`{}`))
			return err
		},
		"a saved search with no name": func() error {
			_, err := s.SaveSearch(ctx, "org-a", "user-1", "", json.RawMessage(`{}`))
			return err
		},
		"a saved search that is a list": func() error {
			_, err := s.SaveSearch(ctx, "org-a", "user-1", "Drift", json.RawMessage(`[]`))
			return err
		},
		"an actor id that is a name": func() error {
			_, err := s.SetStage(ctx, "org-a", "Kari Nordmann", "2026-116131", "inbox")
			return err
		},
	} {
		if err := fn(); !errors.Is(err, ErrInvalid) {
			t.Errorf("%s: err %v, want ErrInvalid", name, err)
		}
	}
}

func TestEveryPipelineStageTheAppShowsIsKept(t *testing.T) {
	for _, stage := range []string{"inbox", "review", "working", "sent", "hidden"} {
		if !Stages[stage] {
			t.Errorf("stage %q is not kept", stage)
		}
	}
}
