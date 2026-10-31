package activities

import (
	"os"
	"regexp"
	"strconv"
	"strings"
	"testing"
	"time"
)

// agentLaneStepSource is the workflow file that sets the agent-lane activity's HeartbeatTimeout.
// cmd/workflows imports this package, so this test cannot import that value; it reads the literal
// as source text instead.
const agentLaneStepSource = "../workflows/step_loop.go"

// agentLaneStepFunc names the function that builds the agent-lane activity options. The search is
// limited to its body so a HeartbeatTimeout elsewhere in the file cannot stand in for it.
const agentLaneStepFunc = "executeScheduledAgentStep"

// heartbeatTimeoutExpr matches the HeartbeatTimeout forms the agent lane uses: a bare unit such as
// time.Minute, or a whole count of a unit such as 90 * time.Second. The trailing [,}] keeps a
// longer expression such as time.Minute * 2 from being read as a bare minute.
var heartbeatTimeoutExpr = regexp.MustCompile(`HeartbeatTimeout:\s*(?:(\d+)\s*\*\s*)?time\.(Minute|Second)\s*[,}]`)

// agentLaneHeartbeatTimeout reads the agent-lane HeartbeatTimeout from the workflow source. Any other
// form fails the test, so rewriting the timeout cannot quietly switch the pin off.
func agentLaneHeartbeatTimeout(t *testing.T) time.Duration {
	t.Helper()
	raw, err := os.ReadFile(agentLaneStepSource)
	if err != nil {
		t.Fatalf("read %s: %v", agentLaneStepSource, err)
	}
	src := string(raw)
	start := strings.Index(src, "func "+agentLaneStepFunc+"(")
	if start < 0 {
		t.Fatalf("func %s not found in %s: the agent-lane step moved or was renamed; re-point agentLaneStepFunc in heartbeat_interval_test.go", agentLaneStepFunc, agentLaneStepSource)
	}
	body := src[start:]
	// gofmt puts a top-level function's closing brace at column 0, so the first "\n}" ends the body.
	if end := strings.Index(body, "\n}"); end >= 0 {
		body = body[:end]
	}
	m := heartbeatTimeoutExpr.FindStringSubmatch(body)
	if m == nil {
		t.Fatalf("no HeartbeatTimeout written as time.Minute or N * time.Second in %s in %s: the agent-lane options changed form; re-point heartbeatTimeoutExpr in heartbeat_interval_test.go", agentLaneStepFunc, agentLaneStepSource)
	}
	count := 1
	if m[1] != "" {
		count, err = strconv.Atoi(m[1])
		if err != nil {
			t.Fatalf("HeartbeatTimeout count %q in %s is not an integer: %v", m[1], agentLaneStepSource, err)
		}
	}
	unit := time.Minute
	if m[2] == "Second" {
		unit = time.Second
	}
	return time.Duration(count) * unit
}

// TestScheduledStepHeartbeatStaysInsideAgentLaneTimeout pins the scheduled-step heartbeat ticker to
// at most a third of the agent-lane HeartbeatTimeout. A ticker equal to the timeout sends its first
// heartbeat as the timeout expires, so a step running longer than the timeout is reclaimed as stuck.
func TestScheduledStepHeartbeatStaysInsideAgentLaneTimeout(t *testing.T) {
	timeout := agentLaneHeartbeatTimeout(t)
	if timeout <= 0 {
		t.Fatalf("agent-lane HeartbeatTimeout in %s parsed as %v; it must be positive", agentLaneStepSource, timeout)
	}
	if scheduledStepHeartbeatEvery <= 0 {
		t.Fatalf("scheduledStepHeartbeatEvery = %v; the heartbeat ticker needs a positive interval", scheduledStepHeartbeatEvery)
	}
	// Multiply the interval rather than divide the timeout, so the comparison has no rounding.
	if 3*scheduledStepHeartbeatEvery > timeout {
		t.Fatalf("scheduledStepHeartbeatEvery = %v is more than a third of the agent-lane HeartbeatTimeout %v in %s; lower the interval so a delayed heartbeat cannot reach the timeout", scheduledStepHeartbeatEvery, timeout, agentLaneStepSource)
	}
}
