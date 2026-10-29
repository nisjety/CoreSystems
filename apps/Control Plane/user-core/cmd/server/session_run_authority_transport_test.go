package main

import "testing"

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
