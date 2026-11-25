package server

import (
	"errors"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
)

// O-19 (owner decision D-f09-3): the org's web.covers_browser decides whether its browser tools follow web.mode. The gate
// withholds a browser capability under web.mode only while the browser switch is on and the org covers its browser tools
// (web.covers_browser true, the default; false is the opt-out). The public-web capabilities, web_search among them
// (cap.tool.http), follow web.mode whatever the org chose for the browser tools. An unreadable policy fails closed to covers.

// orgWebMatrixIDs are the three browser capabilities and web_search, which is cap.tool.http.
func orgWebMatrixIDs() []string {
	return append(append([]string{}, orgWebBrowserTestIDs...), "cap.tool.http")
}

// The matrix is web.mode x web.covers_browser x the browser switch, for each capability it names.
func TestOrgWebModeCoversBrowserMatrix(t *testing.T) {
	for _, mode := range []string{"live", "off", "cached_only"} {
		for _, covers := range []bool{true, false} {
			for _, browserSwitch := range []bool{true, false} {
				reader := &fakeOrgWebModes{mode: mode, covers: covers}
				server, _ := orgWebGateServer(t, reader, true, browserSwitch)
				for _, id := range orgWebMatrixIDs() {
					withheld := mode != "live" && (id == "cap.tool.http" || (browserSwitch && covers))
					response := evaluateOrgWebCapability(t, server, id)
					if withheld {
						if response.Decision != policy.DecisionDeny || response.Reason != orgWebOffReason || response.PolicySource != policy.PolicySourceOrgWebMode {
							t.Fatalf("%s under web.mode %q, covers %v, browser switch %v: decision=%q reason=%q source=%q, want the web-off denial",
								id, mode, covers, browserSwitch, response.Decision, response.Reason, response.PolicySource)
						}
						continue
					}
					if response.Decision != policy.DecisionAllow {
						t.Fatalf("%s under web.mode %q, covers %v, browser switch %v: decision=%q reason=%q, want the allow it had",
							id, mode, covers, browserSwitch, response.Decision, response.Reason)
					}
				}
			}
		}
	}
}

// A policy that cannot be read withholds the public-web set with org_web_unavailable, and the browser set too while its
// switch is on (the org's choice cannot be known, so it covers). Nothing else is touched, and the reader is never read
// for a capability neither set names.
func TestOrgWebModeCoversBrowserUnreadableFailsClosedToCovers(t *testing.T) {
	for _, browserSwitch := range []bool{true, false} {
		reader := &fakeOrgWebModes{err: errors.New("org agent policy unreadable")}
		server, _ := orgWebGateServer(t, reader, true, browserSwitch)
		for _, id := range orgWebMatrixIDs() {
			gated := id == "cap.tool.http" || browserSwitch
			response := evaluateOrgWebCapability(t, server, id)
			if gated {
				if response.Decision != policy.DecisionDeny || response.Reason != orgWebUnavailableReason || response.PolicySource != policy.PolicySourceOrgWebMode {
					t.Fatalf("%s with an unreadable policy, browser switch %v: decision=%q reason=%q source=%q, want org_web_unavailable",
						id, browserSwitch, response.Decision, response.Reason, response.PolicySource)
				}
				continue
			}
			if response.Decision != policy.DecisionAllow {
				t.Fatalf("%s with an unreadable policy and the browser switch off: decision=%q reason=%q, want the allow it had",
					id, response.Decision, response.Reason)
			}
		}
		reads := reader.calls
		if other := evaluateOrgWebCapability(t, server, orgWebOtherTestID); other.Decision != policy.DecisionAllow {
			t.Fatalf("%s with an unreadable policy: decision=%q, want the allow it had", orgWebOtherTestID, other.Decision)
		}
		if reader.calls != reads {
			t.Fatalf("%s read the org's policy (%d reads); a capability neither set names must not be read", orgWebOtherTestID, reader.calls-reads)
		}
	}
}

// With the gate off, nothing is withheld, whatever the org's keys say and whichever switch is on, and the policy is not read.
func TestOrgWebModeCoversBrowserGateOffWithholdsNothing(t *testing.T) {
	for _, browserSwitch := range []bool{true, false} {
		reader := &fakeOrgWebModes{mode: "off", covers: true}
		server, _ := orgWebGateServer(t, reader, false, browserSwitch)
		for _, id := range orgWebMatrixIDs() {
			if response := evaluateOrgWebCapability(t, server, id); response.Decision != policy.DecisionAllow {
				t.Fatalf("%s with the gate off (browser switch %v): decision=%q reason=%q, want the allow it had",
					id, browserSwitch, response.Decision, response.Reason)
			}
		}
		if reader.calls != 0 {
			t.Fatalf("the gate is off but the policy was read %d times", reader.calls)
		}
	}
}
