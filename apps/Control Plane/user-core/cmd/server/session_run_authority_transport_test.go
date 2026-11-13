package main

import (
	"os"
	"testing"
)

func TestSessionRunActionAuthorityTransportFromEnvironmentRejectsInvalidDevelopmentOptIn(t *testing.T) {
	t.Setenv("SESSION_CORE_RUN_AUTHORITY_TLS_CA_FILE", "/tmp/session-core-ca.pem")
	t.Setenv("SESSION_CORE_RUN_AUTHORITY_TLS_SERVER_NAME", "session-core.internal")
	t.Setenv("CONTROL_ALLOW_INSECURE_SESSION_RUN_AUTHORITY_LOOPBACK", "true")
	transport, err := sessionRunActionAuthorityTransportFromEnvironment()
	if err != nil {
		t.Fatalf("sessionRunActionAuthorityTransportFromEnvironment() error = %v", err)
	}
	if transport.TLSCAFile != "/tmp/session-core-ca.pem" || transport.TLSServerName != "session-core.internal" || !transport.AllowInsecureLoopback {
		t.Fatalf("transport = %#v", transport)
	}

	t.Setenv("CONTROL_ALLOW_INSECURE_SESSION_RUN_AUTHORITY_LOOPBACK", "not-a-bool")
	if _, err := sessionRunActionAuthorityTransportFromEnvironment(); err == nil {
		t.Fatal("invalid loopback development opt-in unexpectedly accepted")
	}
}

// TestSessionRunActionAuthorityLoopbackOptInIsStrictLiteral pins the strict literal. Only
// "true" turns the plaintext loopback opt-in on. The off spellings read as off with no error,
// and every other spelling of true is refused instead of being read as on.
func TestSessionRunActionAuthorityLoopbackOptInIsStrictLiteral(t *testing.T) {
	const name = "CONTROL_ALLOW_INSECURE_SESSION_RUN_AUTHORITY_LOOPBACK"

	t.Run("unset", func(t *testing.T) {
		t.Setenv(name, "")
		if err := os.Unsetenv(name); err != nil {
			t.Fatalf("unset %s: %v", name, err)
		}
		transport, err := sessionRunActionAuthorityTransportFromEnvironment()
		if err != nil || transport.AllowInsecureLoopback {
			t.Fatalf("unset: transport %#v, err %v; want off without error", transport, err)
		}
	})
	for _, raw := range []string{"false", "0", "f", "F", "FALSE", "False"} {
		t.Run("off="+raw, func(t *testing.T) {
			t.Setenv(name, raw)
			transport, err := sessionRunActionAuthorityTransportFromEnvironment()
			if err != nil || transport.AllowInsecureLoopback {
				t.Fatalf("%s=%q: transport %#v, err %v; want off without error", name, raw, transport, err)
			}
		})
	}
	t.Run("on=true", func(t *testing.T) {
		t.Setenv(name, "true")
		transport, err := sessionRunActionAuthorityTransportFromEnvironment()
		if err != nil || !transport.AllowInsecureLoopback {
			t.Fatalf("%s=true: transport %#v, err %v; want on", name, transport, err)
		}
	})
	for _, raw := range []string{"1", "t", "T", "TRUE", "True", "yes", "on", "enabled"} {
		t.Run("refused="+raw, func(t *testing.T) {
			t.Setenv(name, raw)
			transport, err := sessionRunActionAuthorityTransportFromEnvironment()
			if err == nil {
				t.Fatalf("%s=%q accepted; only the literal true may turn the opt-in on", name, raw)
			}
			if transport.AllowInsecureLoopback {
				t.Fatalf("%s=%q turned the opt-in on", name, raw)
			}
		})
	}
}
