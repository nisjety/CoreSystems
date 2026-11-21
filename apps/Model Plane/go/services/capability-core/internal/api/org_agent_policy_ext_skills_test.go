package api

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
)

// orgAgentDocRow is one stored org agent policy row as the registry returns it: the revision, the
// document text, its digest, the actor and the update time.
type orgAgentDocRow struct{ doc string }

func (r orgAgentDocRow) Scan(dest ...any) error {
	*dest[0].(*int64) = 3
	*dest[1].(*string) = r.doc
	*dest[2].(*string) = "digest"
	*dest[3].(*string) = "user-admin"
	*dest[4].(**time.Time) = nil
	return nil
}

// TestOrgAgentPolicyExtSkillsWordReadsTheStatedModeAndFailsClosed pins the word a scheduled run carries
// (P2-ec-01-rel1) to the gateway's own reading of the key: the schema default with no row or no key, the
// stated mode when it is a word this build knows, and the fail-closed "none" for anything else.
func TestOrgAgentPolicyExtSkillsWordReadsTheStatedModeAndFailsClosed(t *testing.T) {
	cases := []struct {
		name string
		row  pgx.Row
		want string
	}{
		{"no row takes the schema default", orgAgentErrRow{err: pgx.ErrNoRows}, "any"},
		{"a document without the key takes the default", orgAgentDocRow{doc: `{"mode_cap":"auto"}`}, "any"},
		{"approved_only is read", orgAgentDocRow{doc: `{"ext.skills":{"mode":"approved_only","ids":[]}}`}, "approved_only"},
		{"none is read", orgAgentDocRow{doc: `{"ext.skills":{"mode":"none","ids":[]}}`}, "none"},
		{"any is read", orgAgentDocRow{doc: `{"ext.skills":{"mode":"any","ids":[]}}`}, "any"},
		{"an unknown mode fails closed", orgAgentDocRow{doc: `{"ext.skills":{"mode":"everyone","ids":[]}}`}, "none"},
		{"a value without a mode fails closed", orgAgentDocRow{doc: `{"ext.skills":{"ids":[]}}`}, "none"},
		{"a bare word fails closed", orgAgentDocRow{doc: `{"ext.skills":"any"}`}, "none"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			handler := NewOrgAgentPolicyHandler(orgAgentFakeDB{t: t, row: tc.row}, false)
			got, err := handler.ExtSkillsWord(context.Background(), "org-a")
			if err != nil {
				t.Fatalf("ExtSkillsWord: %v", err)
			}
			if got != tc.want {
				t.Fatalf("ExtSkillsWord = %q, want %q", got, tc.want)
			}
		})
	}

	t.Run("an unreadable registry is an error, never a word", func(t *testing.T) {
		handler := NewOrgAgentPolicyHandler(orgAgentFakeDB{t: t, row: orgAgentErrRow{err: errors.New("connection refused")}}, false)
		if got, err := handler.ExtSkillsWord(context.Background(), "org-a"); err == nil {
			t.Fatalf("ExtSkillsWord = %q with no error; an unreadable policy must be refused", got)
		}
	})
}
