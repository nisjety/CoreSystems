package taskexec

import (
	"context"
	"errors"
	"testing"

	"google.golang.org/protobuf/types/known/structpb"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/cron"
)

// recordingOrgExtSkills is the org policy reader a dispatcher test uses: it answers one word or error and
// records the orgs it was asked about.
type recordingOrgExtSkills struct {
	word  string
	err   error
	asked []string
}

func (r *recordingOrgExtSkills) ExtSkillsWord(_ context.Context, orgID string) (string, error) {
	r.asked = append(r.asked, orgID)
	return r.word, r.err
}

// TestScheduledRunStatesTheOrgsExtSkillsWordOnlyWhileEnforced pins P2-ec-01-rel1 at the dispatcher. With
// the switch off nothing is read or stated. With it on, the word is read for the fire's org, and a policy
// that cannot be read refuses the fire rather than starting it unheld.
func TestScheduledRunStatesTheOrgsExtSkillsWordOnlyWhileEnforced(t *testing.T) {
	ctx := context.Background()
	dispatcher := &WorkflowDispatcher{}

	off := &recordingOrgExtSkills{word: "approved_only"}
	dispatcher.SetOrgExtSkills(off, false)
	word, err := dispatcher.scheduledExtSkillsWord(ctx, "org-1")
	if err != nil || word != "" {
		t.Fatalf("switch off: word %q, error %v; want the empty word and no error", word, err)
	}
	if len(off.asked) != 0 {
		t.Fatalf("switch off read the org policy %d time(s); want none", len(off.asked))
	}

	on := &recordingOrgExtSkills{word: "approved_only"}
	dispatcher.SetOrgExtSkills(on, true)
	word, err = dispatcher.scheduledExtSkillsWord(ctx, "org-1")
	if err != nil || word != "approved_only" {
		t.Fatalf("switch on: word %q, error %v; want approved_only", word, err)
	}
	if len(on.asked) != 1 || on.asked[0] != "org-1" {
		t.Fatalf("org policy was asked for %v; want the fire's org only", on.asked)
	}

	unreadable := &recordingOrgExtSkills{err: errors.New("org agent policy read failed")}
	dispatcher.SetOrgExtSkills(unreadable, true)
	if _, err := dispatcher.scheduledExtSkillsWord(ctx, "org-1"); err == nil {
		t.Fatal("an unreadable org policy must refuse the fire, not start it unheld")
	}

	dispatcher.SetOrgExtSkills(nil, true)
	if _, err := dispatcher.scheduledExtSkillsWord(ctx, "org-1"); err == nil {
		t.Fatal("an enforced switch with no policy reader must refuse the fire, not start it unheld")
	}
}

// TestScheduledFireStatesTheOrgsExtSkillsWordBeforeThePreparation runs the fire's reauthorization end to
// end with fakes. The word is carried to the prepared handoff; an unreadable policy refuses the fire before
// Session Core prepares any thread; and with the switch off the handoff carries no word.
func TestScheduledFireStatesTheOrgsExtSkillsWordBeforeThePreparation(t *testing.T) {
	run := func(t *testing.T, reader OrgExtSkillsReader, enforced bool) (*preparedScheduledRun, *recordingScheduledRunSession, error) {
		t.Helper()
		intent := cron.FireIntent{
			OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", ScheduleID: "schedule-1",
			FireKey:        "2026-08-14T00:00:00Z",
			IdempotencyKey: "schedule-1:2026-08-14T00:00:00Z",
		}
		config, _ := scheduledTaskConfig(t, map[string]any{
			"workflow_input": map[string]any{"goal": "scheduled work", "policy": "execute"},
		}, intent)
		authorizer := &recordingScheduledRunAuthorizer{preparation: cron.ScheduledRunPreparation{
			Token: "one-fire-control-bearer", SystemThreadKey: "schedule/schedule-1/2026-08-14T00:00:00Z",
		}}
		session := &recordingScheduledRunSession{response: &mpv1.PrepareScheduledRunThreadResponse{
			ThreadId: "thread-scheduled-1", RunId: "task-1", OwnerId: "service:orchestrator-core",
		}}
		dispatcher := &WorkflowDispatcher{fireAuthorizer: authorizer, scheduledRunSession: session}
		dispatcher.SetOrgExtSkills(reader, enforced)
		prepared, err := dispatcher.reauthorizeScheduleFire(context.Background(), TaskRef{ID: "task-1", OrgID: "org-1"}, taskDetail{
			config: config, scheduleID: "schedule-1",
		})
		return prepared, session, err
	}

	stated := &recordingOrgExtSkills{word: "approved_only"}
	prepared, session, err := run(t, stated, true)
	if err != nil {
		t.Fatalf("reauthorizeScheduleFire: %v", err)
	}
	if prepared == nil || prepared.extSkills != "approved_only" {
		t.Fatalf("prepared handoff carries ext.skills %q; want approved_only", prepared.extSkills)
	}
	if session.request == nil {
		t.Fatal("Session Core was not asked to prepare the thread for a readable policy")
	}
	if len(stated.asked) != 1 || stated.asked[0] != "org-1" {
		t.Fatalf("org policy was asked for %v; want the fire's org only", stated.asked)
	}

	unreadable := &recordingOrgExtSkills{err: errors.New("org agent policy read failed")}
	_, session, err = run(t, unreadable, true)
	if err == nil {
		t.Fatal("an unreadable org policy must refuse the fire")
	}
	if session.request != nil {
		t.Fatal("an unreadable org policy reached Session Core before the fire was refused")
	}

	off := &recordingOrgExtSkills{word: "approved_only"}
	prepared, _, err = run(t, off, false)
	if err != nil {
		t.Fatalf("reauthorizeScheduleFire with the switch off: %v", err)
	}
	if prepared == nil || prepared.extSkills != "" {
		t.Fatalf("switch off carries ext.skills %q; want none", prepared.extSkills)
	}
	if len(off.asked) != 0 {
		t.Fatalf("switch off read the org policy %d time(s); want none", len(off.asked))
	}
}

// TestAttachScheduledRunInputStatesOnlyTheDispatchersExtSkillsWord pins the workflow input. The word the
// dispatcher read is stated on it and replaces any copy a task template carries. With no word read, a
// template that carries one is refused, as it was before the field existed, and a template without one
// leaves the input exactly as it was.
func TestAttachScheduledRunInputStatesOnlyTheDispatchersExtSkillsWord(t *testing.T) {
	prepared := preparedScheduledRun{
		threadID: "thread-scheduled-1", spaceRef: "space-1", subjectID: "user-1",
		scheduleID: "schedule-1", fireKey: "2026-08-14T00:00:00Z",
		templateDigest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		templateJSON:   `{"workflow_input":{"goal":"Nightly sweep"}}`,
		idempotencyKey: "schedule-1:2026-08-14T00:00:00Z",
	}
	// build attaches the prepared handoff to a task whose template carries the word "any" when
	// templateCarries is set, and returns the workflow input or the refusal.
	build := func(t *testing.T, word string, templateCarries bool) (map[string]any, error) {
		t.Helper()
		req, err := dispatchPlan(
			TaskRef{ID: "task-1", OrgID: "org-1"},
			detail("running", "org-1", "Nightly sweep", "", ""),
			"",
		)
		if err != nil {
			t.Fatalf("dispatchPlan: %v", err)
		}
		if templateCarries {
			req.Input.Fields["ext_skills"] = structpb.NewStringValue("any")
		}
		fire := prepared
		fire.extSkills = word
		if err := attachScheduledRunInput(req, fire, "task-1"); err != nil {
			return nil, err
		}
		return req.GetInput().AsMap(), nil
	}

	stated, err := build(t, "approved_only", true)
	if err != nil {
		t.Fatalf("a stated word must replace the template's copy: %v", err)
	}
	if got := stated["ext_skills"]; got != "approved_only" {
		t.Fatalf("ext_skills = %#v; want the word the dispatcher read", got)
	}

	if _, err := build(t, "", true); err == nil {
		t.Fatal("a template that carries ext_skills must be refused while no word is stated")
	}

	plain, err := build(t, "", false)
	if err != nil {
		t.Fatalf("a template without ext_skills and no stated word: %v", err)
	}
	if _, found := plain["ext_skills"]; found {
		t.Fatalf("the input carries ext_skills with no word stated: %#v", plain)
	}
}
