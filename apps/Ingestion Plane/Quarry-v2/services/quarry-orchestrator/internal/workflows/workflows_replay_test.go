package workflows

// W2-U60 (D2-071, D2-139): the replay-before-deploy gate for CrawlJobWF.
//
// crawl-scope-v1 (WS14.S3) changed the command sequence of a crawl that carries a scope. A
// crawl that started before it, and is still running when the new worker deploys, must
// replay on the new code with no non-determinism error. This file replays recorded
// histories of CrawlJobWF through Temporal's WorkflowReplayer:
//
//   - pre_* fixtures are crawls recorded before crawl-scope-v1 (and, for pre_bounded, before
//     crawl-bounded-v1): no scope marker in the history, so GetVersion answers DefaultVersion.
//   - scoped_* fixtures are crawls recorded with crawl-scope-v1 on, so the next change to the
//     scope branch cannot break the crawls that run under it now.
//
// The fixtures are in testdata/replay. They were recorded with the test environment: the
// workflow ran under TestWorkflowEnvironment with GetVersion pinned to the version the
// scenario names, and every activity the workflow scheduled, in order, became the events of
// a history (recordHistory below). The test environment keeps no history of its own, and no
// live Temporal was available to export one from; so a fixture is the command sequence
// the code under test produces, written out as the events a server would record. A history
// exported from a live Temporal (`temporal workflow show --output json`) can be dropped into
// testdata/replay as exported_*.json (or exported_durable_*.json for the durable frontier,
// which production runs) and this test replays it too.
//
// Regenerate the fixtures with:
//
//	QUARRY_RECORD_REPLAY=1 go test ./internal/workflows -run TestRecordReplayFixtures
//
// Do it only to add a scenario, never to make a failing replay pass.

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"
	"time"

	"github.com/stretchr/testify/require"
	commonpb "go.temporal.io/api/common/v1"
	enumspb "go.temporal.io/api/enums/v1"
	historypb "go.temporal.io/api/history/v1"
	taskqueuepb "go.temporal.io/api/taskqueue/v1"
	"go.temporal.io/api/temporalproto"
	"go.temporal.io/sdk/activity"
	"go.temporal.io/sdk/client"
	"go.temporal.io/sdk/converter"
	"go.temporal.io/sdk/testsuite"
	"go.temporal.io/sdk/worker"
	"go.temporal.io/sdk/workflow"
	"google.golang.org/protobuf/types/known/durationpb"
	"google.golang.org/protobuf/types/known/timestamppb"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
)

const replayFixtureDir = "testdata/replay"

// replayScenario is one recorded crawl.
type replayScenario struct {
	name string
	// durable runs the durable frontier, the production path.
	durable bool
	// markers are the GetVersion ids that answer 1 when the history is recorded. Every id
	// not listed answers DefaultVersion, as it does for an execution recorded before it.
	markers []string
	input   CrawlJobInput
}

func replayScenarios() []replayScenario {
	plain := func(runID string) CrawlJobInput {
		return CrawlJobInput{
			RunID: runID, JobID: "job_" + runID, OrgID: "org_replay",
			Seeds: []string{sitemapRoot}, MaxDepth: 3, MaxPages: 20,
		}
	}
	scoped := func(runID string) CrawlJobInput {
		in := plain(runID)
		in.Scope = &activities.CrawlScope{CrawlEntireDomain: true}
		in.ScopeMode = activities.ScopeModeEnforce
		return in
	}
	return []replayScenario{
		{name: "pre_bounded_inmemory", input: plain("run_replay_pre_bounded")},
		{name: "pre_scope_inmemory", markers: []string{"crawl-bounded-v1"}, input: plain("run_replay_pre_scope")},
		{name: "pre_scope_durable", durable: true, markers: []string{"crawl-bounded-v1"}, input: plain("run_replay_pre_scope_durable")},
		{name: "scoped_inmemory", markers: []string{"crawl-bounded-v1", "crawl-scope-v1"}, input: scoped("run_replay_scoped")},
		{name: "scoped_durable", durable: true, markers: []string{"crawl-bounded-v1", "crawl-scope-v1"}, input: scoped("run_replay_scoped_durable")},
	}
}

// recordedActivity is one completed activity of a recorded run.
type recordedActivity struct {
	name   string
	result json.RawMessage
}

// recordHistory runs the scenario under the test environment and writes the activities it
// scheduled out as a Temporal history.
func recordHistory(t *testing.T, sc replayScenario) *historypb.History {
	t.Helper()
	suite := &testsuite.WorkflowTestSuite{}
	env := suite.NewTestWorkflowEnvironment()
	a := activities.New(activities.Config{})
	a.DurableFrontier = sc.durable
	env.RegisterActivity(a)

	marked := map[string]bool{}
	for _, id := range sc.markers {
		marked[id] = true
	}
	for _, id := range []string{"crawl-bounded-v1", "crawl-scope-v1"} {
		if marked[id] {
			env.OnGetVersion(id, workflow.DefaultVersion, 1).Return(workflow.Version(1))
		} else {
			env.OnGetVersion(id, workflow.DefaultVersion, 1).Return(workflow.DefaultVersion)
		}
	}

	site := newSitemapSite()
	site.links[sitemapRoot] = []string{"https://site.test/a", "https://site.test/p2"}
	baseMocks(env, a, site, &eventLog{})
	if sc.durable {
		durableMocks(env, a)
	}

	var recorded []recordedActivity
	env.SetOnActivityCompletedListener(func(info *activity.Info, result converter.EncodedValue, err error) {
		require.NoError(t, err, "a recorded scenario has no failing activity")
		var raw json.RawMessage
		if result != nil && result.HasValue() {
			require.NoError(t, result.Get(&raw))
		}
		recorded = append(recorded, recordedActivity{name: info.ActivityType.Name, result: raw})
	})

	env.ExecuteWorkflow(CrawlJobWF, sc.input, a)
	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	require.NotEmpty(t, recorded)

	input, err := converter.GetDefaultDataConverter().ToPayloads(sc.input)
	require.NoError(t, err)
	return buildHistory(t, input, sc.markers, recorded)
}

// buildHistory writes a serial workflow as the events a server records for it: one
// workflow task per activity, the version markers in the first one.
func buildHistory(t *testing.T, input *commonpb.Payloads, markers []string, recorded []recordedActivity) *historypb.History {
	t.Helper()
	dc := converter.GetDefaultDataConverter()
	tq := &taskqueuepb.TaskQueue{Name: "quarry-orchestrator", Kind: enumspb.TASK_QUEUE_KIND_NORMAL}
	clock := time.Date(2026, time.January, 5, 9, 0, 0, 0, time.UTC)
	var events []*historypb.HistoryEvent
	add := func(kind enumspb.EventType) *historypb.HistoryEvent {
		clock = clock.Add(time.Second)
		e := &historypb.HistoryEvent{
			EventId:   int64(len(events) + 1),
			EventTime: timestamppb.New(clock),
			EventType: kind,
		}
		events = append(events, e)
		return e
	}

	started := add(enumspb.EVENT_TYPE_WORKFLOW_EXECUTION_STARTED)
	started.Attributes = &historypb.HistoryEvent_WorkflowExecutionStartedEventAttributes{
		WorkflowExecutionStartedEventAttributes: &historypb.WorkflowExecutionStartedEventAttributes{
			WorkflowType:             &commonpb.WorkflowType{Name: "CrawlJobWF"},
			TaskQueue:                tq,
			Input:                    input,
			WorkflowTaskTimeout:      durationpb.New(10 * time.Second),
			WorkflowRunTimeout:       durationpb.New(24 * time.Hour),
			WorkflowExecutionTimeout: durationpb.New(24 * time.Hour),
			OriginalExecutionRunId:   "replay-fixture-run",
			FirstExecutionRunId:      "replay-fixture-run",
			Attempt:                  1,
			Identity:                 "replay-fixture",
		},
	}

	// workflowTask appends the three events of one workflow task and returns the id of the
	// completed one, which the commands of the task point at.
	workflowTask := func() int64 {
		sched := add(enumspb.EVENT_TYPE_WORKFLOW_TASK_SCHEDULED)
		sched.Attributes = &historypb.HistoryEvent_WorkflowTaskScheduledEventAttributes{
			WorkflowTaskScheduledEventAttributes: &historypb.WorkflowTaskScheduledEventAttributes{
				TaskQueue: tq, StartToCloseTimeout: durationpb.New(10 * time.Second), Attempt: 1,
			},
		}
		start := add(enumspb.EVENT_TYPE_WORKFLOW_TASK_STARTED)
		start.Attributes = &historypb.HistoryEvent_WorkflowTaskStartedEventAttributes{
			WorkflowTaskStartedEventAttributes: &historypb.WorkflowTaskStartedEventAttributes{
				ScheduledEventId: sched.EventId, Identity: "replay-fixture", RequestId: fmt.Sprintf("req-%d", start.GetEventId()),
			},
		}
		done := add(enumspb.EVENT_TYPE_WORKFLOW_TASK_COMPLETED)
		done.Attributes = &historypb.HistoryEvent_WorkflowTaskCompletedEventAttributes{
			WorkflowTaskCompletedEventAttributes: &historypb.WorkflowTaskCompletedEventAttributes{
				ScheduledEventId: sched.EventId, StartedEventId: start.EventId, Identity: "replay-fixture",
			},
		}
		return done.EventId
	}

	for i, act := range recorded {
		completedTask := workflowTask()
		if i == 0 {
			// The first task carries the version markers, each followed by the
			// search attribute update the SDK records with it.
			seen := []string{}
			for _, id := range markers {
				seen = append(seen, fmt.Sprintf("%s-1", id))
				idPayload, err := dc.ToPayloads(id)
				require.NoError(t, err)
				versionPayload, err := dc.ToPayloads(1)
				require.NoError(t, err)
				marker := add(enumspb.EVENT_TYPE_MARKER_RECORDED)
				marker.Attributes = &historypb.HistoryEvent_MarkerRecordedEventAttributes{
					MarkerRecordedEventAttributes: &historypb.MarkerRecordedEventAttributes{
						MarkerName:                   "Version",
						Details:                      map[string]*commonpb.Payloads{"change-id": idPayload, "version": versionPayload},
						WorkflowTaskCompletedEventId: completedTask,
					},
				}
				changeVersions, err := dc.ToPayload(append([]string(nil), seen...))
				require.NoError(t, err)
				changeVersions.Metadata["type"] = []byte("Keyword")
				upsert := add(enumspb.EVENT_TYPE_UPSERT_WORKFLOW_SEARCH_ATTRIBUTES)
				upsert.Attributes = &historypb.HistoryEvent_UpsertWorkflowSearchAttributesEventAttributes{
					UpsertWorkflowSearchAttributesEventAttributes: &historypb.UpsertWorkflowSearchAttributesEventAttributes{
						WorkflowTaskCompletedEventId: completedTask,
						SearchAttributes: &commonpb.SearchAttributes{
							IndexedFields: map[string]*commonpb.Payload{"TemporalChangeVersion": changeVersions},
						},
					},
				}
			}
		}
		sched := add(enumspb.EVENT_TYPE_ACTIVITY_TASK_SCHEDULED)
		sched.Attributes = &historypb.HistoryEvent_ActivityTaskScheduledEventAttributes{
			ActivityTaskScheduledEventAttributes: &historypb.ActivityTaskScheduledEventAttributes{
				ActivityId:                   fmt.Sprintf("%d", sched.EventId),
				ActivityType:                 &commonpb.ActivityType{Name: act.name},
				TaskQueue:                    tq,
				ScheduleToCloseTimeout:       durationpb.New(0),
				ScheduleToStartTimeout:       durationpb.New(0),
				StartToCloseTimeout:          durationpb.New(5 * time.Minute),
				WorkflowTaskCompletedEventId: completedTask,
			},
		}
		begun := add(enumspb.EVENT_TYPE_ACTIVITY_TASK_STARTED)
		begun.Attributes = &historypb.HistoryEvent_ActivityTaskStartedEventAttributes{
			ActivityTaskStartedEventAttributes: &historypb.ActivityTaskStartedEventAttributes{
				ScheduledEventId: sched.EventId, Identity: "replay-fixture", RequestId: fmt.Sprintf("act-%d", begun.EventId), Attempt: 1,
			},
		}
		var result *commonpb.Payloads
		if len(act.result) > 0 {
			var err error
			result, err = dc.ToPayloads(act.result)
			require.NoError(t, err)
		}
		finished := add(enumspb.EVENT_TYPE_ACTIVITY_TASK_COMPLETED)
		finished.Attributes = &historypb.HistoryEvent_ActivityTaskCompletedEventAttributes{
			ActivityTaskCompletedEventAttributes: &historypb.ActivityTaskCompletedEventAttributes{
				Result: result, ScheduledEventId: sched.EventId, StartedEventId: begun.EventId, Identity: "replay-fixture",
			},
		}
	}

	last := workflowTask()
	done := add(enumspb.EVENT_TYPE_WORKFLOW_EXECUTION_COMPLETED)
	done.Attributes = &historypb.HistoryEvent_WorkflowExecutionCompletedEventAttributes{
		WorkflowExecutionCompletedEventAttributes: &historypb.WorkflowExecutionCompletedEventAttributes{
			WorkflowTaskCompletedEventId: last,
		},
	}
	return &historypb.History{Events: events}
}

// TestRecordReplayFixtures writes the fixtures. It does nothing unless asked to.
func TestRecordReplayFixtures(t *testing.T) {
	if os.Getenv("QUARRY_RECORD_REPLAY") != "1" {
		t.Skip("set QUARRY_RECORD_REPLAY=1 to re-record testdata/replay")
	}
	require.NoError(t, os.MkdirAll(replayFixtureDir, 0o755))
	for _, sc := range replayScenarios() {
		history := recordHistory(t, sc)
		raw, err := temporalproto.CustomJSONMarshalOptions{Indent: "  "}.Marshal(history)
		require.NoError(t, err)
		raw = append(bytes.TrimRight(raw, "\n"), '\n')
		require.NoError(t, os.WriteFile(filepath.Join(replayFixtureDir, sc.name+".json"), raw, 0o644))
	}
}

// replayerFor registers CrawlJobWF the way the worker does (cmd/orchestrator/main.go):
// a wrapper that injects the activities, registered under the workflow's own name.
func replayerFor(t *testing.T, durable bool) worker.WorkflowReplayer {
	t.Helper()
	acts := activities.New(activities.Config{})
	acts.DurableFrontier = durable
	replayer, err := worker.NewWorkflowReplayerWithOptions(worker.WorkflowReplayerOptions{})
	require.NoError(t, err)
	replayer.RegisterWorkflowWithOptions(
		func(ctx workflow.Context, in CrawlJobInput) error { return CrawlJobWF(ctx, in, acts) },
		workflow.RegisterOptions{Name: "CrawlJobWF"},
	)
	return replayer
}

func loadReplayHistory(t *testing.T, path string) *historypb.History {
	t.Helper()
	f, err := os.Open(path)
	require.NoError(t, err)
	defer f.Close()
	history, err := client.HistoryFromJSON(f, client.HistoryJSONOptions{})
	require.NoError(t, err)
	return history
}

// fixtureIsDurable reads the frontier a fixture ran on from its name: production runs the
// durable frontier, an exported history of it is named exported_durable_*.json.
func fixtureIsDurable(name string) bool { return strings.Contains(name, "durable") }

func replayFixtures(t *testing.T) []string {
	t.Helper()
	paths, err := filepath.Glob(filepath.Join(replayFixtureDir, "*.json"))
	require.NoError(t, err)
	sort.Strings(paths)
	require.NotEmpty(t, paths, "testdata/replay holds no history; run TestRecordReplayFixtures with QUARRY_RECORD_REPLAY=1")
	return paths
}

// markersOf lists the change ids of the version markers of a history, in order.
func markersOf(history *historypb.History) []string {
	var ids []string
	for _, e := range history.GetEvents() {
		if e.GetEventType() != enumspb.EVENT_TYPE_MARKER_RECORDED {
			continue
		}
		for _, p := range e.GetMarkerRecordedEventAttributes().GetDetails()["change-id"].GetPayloads() {
			var id string
			if err := converter.GetDefaultDataConverter().FromPayload(p, &id); err == nil {
				ids = append(ids, id)
			}
		}
	}
	return ids
}

// TestReplay_RecordedCrawlHistoriesReplayOnCurrentCrawlJobWF is the gate: every recorded
// history, from before crawl-bounded-v1 to the scoped one, replays against the CrawlJobWF
// this tree builds.
func TestReplay_RecordedCrawlHistoriesReplayOnCurrentCrawlJobWF(t *testing.T) {
	for _, path := range replayFixtures(t) {
		name := strings.TrimSuffix(filepath.Base(path), ".json")
		t.Run(name, func(t *testing.T) {
			history := loadReplayHistory(t, path)
			err := replayerFor(t, fixtureIsDurable(name)).ReplayWorkflowHistory(nil, history)
			require.NoError(t, err, "a crawl recorded before this worker does not replay on it; do not deploy")
		})
	}
}

// TestReplay_PreScopeFixturesCarryNoScopeMarker pins what makes a fixture a pre-scope one,
// so a re-recording that picked up the marker cannot pass for the old history.
func TestReplay_PreScopeFixturesCarryNoScopeMarker(t *testing.T) {
	for _, path := range replayFixtures(t) {
		name := strings.TrimSuffix(filepath.Base(path), ".json")
		markers := markersOf(loadReplayHistory(t, path))
		switch {
		case strings.HasPrefix(name, "pre_bounded"):
			require.Empty(t, markers, "%s predates every version marker", name)
		case strings.HasPrefix(name, "pre_scope"):
			require.Equal(t, []string{"crawl-bounded-v1"}, markers, "%s predates crawl-scope-v1", name)
		case strings.HasPrefix(name, "scoped"):
			require.Equal(t, []string{"crawl-bounded-v1", "crawl-scope-v1"}, markers, "%s runs under crawl-scope-v1", name)
		}
	}
}

// TestReplay_FixturesMatchTheirScenarios keeps the files and replayScenarios in step: the
// committed history of a scenario has the events the test environment records today.
func TestReplay_FixturesMatchTheirScenarios(t *testing.T) {
	for _, sc := range replayScenarios() {
		t.Run(sc.name, func(t *testing.T) {
			committed := loadReplayHistory(t, filepath.Join(replayFixtureDir, sc.name+".json"))
			fresh := recordHistory(t, sc)
			require.Equal(t, eventShape(fresh), eventShape(committed),
				"the command sequence of %s changed since it was recorded; that is exactly what a replay catches", sc.name)
		})
	}
}

func eventShape(h *historypb.History) []string {
	shape := make([]string, 0, len(h.GetEvents()))
	for _, e := range h.GetEvents() {
		line := e.GetEventType().String()
		switch e.GetEventType() {
		case enumspb.EVENT_TYPE_ACTIVITY_TASK_SCHEDULED:
			line += " " + e.GetActivityTaskScheduledEventAttributes().GetActivityType().GetName()
		case enumspb.EVENT_TYPE_MARKER_RECORDED:
			line += " " + strings.Join(markersOf(&historypb.History{Events: []*historypb.HistoryEvent{e}}), ",")
		}
		shape = append(shape, line)
	}
	return shape
}

// TestReplay_ADriftedCommandSequenceIsCaught proves the gate has teeth: the same history with
// one activity renamed, which is what an unversioned change to the loop does, fails.
func TestReplay_ADriftedCommandSequenceIsCaught(t *testing.T) {
	history := loadReplayHistory(t, filepath.Join(replayFixtureDir, "pre_scope_inmemory.json"))
	renamed := false
	for _, e := range history.GetEvents() {
		if e.GetEventType() == enumspb.EVENT_TYPE_ACTIVITY_TASK_SCHEDULED &&
			e.GetActivityTaskScheduledEventAttributes().GetActivityType().GetName() == "RunPage" {
			e.GetActivityTaskScheduledEventAttributes().ActivityType.Name = "RunPageDrifted"
			renamed = true
			break
		}
	}
	require.True(t, renamed)
	err := replayerFor(t, false).ReplayWorkflowHistory(nil, history)
	require.Error(t, err, "a changed command sequence must not replay")
	require.Contains(t, strings.ToLower(err.Error()), "nondeterministic")
}

// TestReplay_AScopedInputOnAPreScopeHistoryRunsTheLegacyLoop is the case crawl-scope-v1
// exists for: a crawl that started before the marker, whose input now names a scope (a
// re-dispatch, or an edge that stamps one on a job already running). Its history has no
// scope marker, so GetVersion answers DefaultVersion and the loop must run as it did. If the
// scope branch were not behind the marker, the replayed pages would be scoped, the recorded
// results carry no scope proof, and the commands would diverge.
func TestReplay_AScopedInputOnAPreScopeHistoryRunsTheLegacyLoop(t *testing.T) {
	for _, name := range []string{"pre_scope_inmemory", "pre_scope_durable", "pre_bounded_inmemory"} {
		t.Run(name, func(t *testing.T) {
			history := loadReplayHistory(t, filepath.Join(replayFixtureDir, name+".json"))
			started := history.Events[0].GetWorkflowExecutionStartedEventAttributes()
			var in CrawlJobInput
			require.NoError(t, converter.GetDefaultDataConverter().FromPayloads(started.Input, &in))
			require.Nil(t, in.Scope)
			in.Scope = &activities.CrawlScope{CrawlEntireDomain: true}
			in.ScopeMode = activities.ScopeModeEnforce
			scoped, err := converter.GetDefaultDataConverter().ToPayloads(in)
			require.NoError(t, err)
			started.Input = scoped

			err = replayerFor(t, fixtureIsDurable(name)).ReplayWorkflowHistory(nil, history)
			require.NoError(t, err, "a crawl without the scope marker must keep the legacy loop whatever its input says")
		})
	}
}
