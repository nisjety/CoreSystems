package codexsubscription

import (
	"context"
	"errors"
	"os"
	"slices"
	"testing"
	"time"
)

type reentrantReadTestLocker struct{ locked bool }

func (l *reentrantReadTestLocker) WithExclusive(_ context.Context, _ string, fn func() error) error {
	if l.locked {
		return errors.New("credential lock re-entered")
	}
	l.locked = true
	defer func() { l.locked = false }()
	return fn()
}

type reentrantReadTestAuthenticator struct{ locker *reentrantReadTestLocker }

func (*reentrantReadTestAuthenticator) BeginDeviceLogin(context.Context, string) (LoginProcess, DeviceCode, error) {
	return nil, DeviceCode{}, nil
}
func (*reentrantReadTestAuthenticator) Logout(context.Context, string) error { return nil }
func (a *reentrantReadTestAuthenticator) ReadRateLimits(ctx context.Context, home string) (RateLimitReport, error) {
	var report RateLimitReport
	err := a.locker.WithExclusive(ctx, home, func() error {
		report = RateLimitReport{ObservedAt: time.Now().UTC()}
		return nil
	})
	return report, err
}
func (*reentrantReadTestAuthenticator) readRateLimitsFromAppServer(context.Context, string) (RateLimitReport, error) {
	return RateLimitReport{ObservedAt: time.Now().UTC()}, nil
}

func TestComposedRunnerRateLimitReadDoesNotReenterCredentialLock(t *testing.T) {
	locker := &reentrantReadTestLocker{}
	runner := NewRunnerWithTransport(
		&reentrantReadTestAuthenticator{locker: locker},
		nil,
		locker,
	)
	report, err := runner.(*composedRunner).ReadRateLimits(context.Background(), "connection-home")
	if err != nil {
		t.Fatalf("ReadRateLimits: %v", err)
	}
	if report.ObservedAt.IsZero() {
		t.Fatal("expected provider snapshot timestamp")
	}
}

func TestParseTransportKind(t *testing.T) {
	for raw, want := range map[string]TransportKind{"": TransportAppServer, "direct": TransportDirect, " Direct ": TransportDirect, "app-server": TransportAppServer, "APP-SERVER": TransportAppServer} {
		got, err := ParseTransportKind(raw)
		if err != nil || got != want {
			t.Fatalf("ParseTransportKind(%q) = %q, %v", raw, got, err)
		}
	}
	if _, err := ParseTransportKind("grpc"); err == nil {
		t.Fatal("grpc must be rejected")
	}
}

func TestNewRunnerSelectsTransport(t *testing.T) {
	for _, kind := range []TransportKind{"", TransportDirect, TransportAppServer} {
		runner, err := NewRunner(RunnerOptions{Transport: kind, AppServerCommand: "codex", AppServerHardened: true, AppServerThreadEffort: true, Direct: DirectConfig{MaxRetries: DefaultDirectMaxRetries}})
		if err != nil {
			t.Fatalf("NewRunner(%q): %v", kind, err)
		}
		composed := runner.(*composedRunner)
		want := kind
		if want == "" {
			want = TransportAppServer
		}
		if composed.TransportKind() != want {
			t.Fatalf("transport = %q, want %q", composed.TransportKind(), want)
		}
		process, ok := composed.auth.(*ProcessRunner)
		if !ok || !process.hardened || process.creds == nil {
			t.Fatal("login must always use the hardened, credential-aware Codex process runner")
		}
		if !process.threadEffort {
			t.Fatal("AppServerThreadEffort=true did not reach the Codex process runner")
		}
		if composed.creds == nil {
			t.Fatal("logout must run under the credential lock")
		}
		manager, err := NewManager(Config{Enabled: true, Home: t.TempDir()}, runner)
		if err != nil || manager.TransportKind() != want {
			t.Fatalf("Manager.TransportKind() = %q, %v", manager.TransportKind(), err)
		}
	}
	runner, err := NewRunner(RunnerOptions{Transport: TransportAppServer, AppServerCommand: "codex", AppServerHardened: true, AppServerThreadEffort: false})
	if err != nil {
		t.Fatal(err)
	}
	if runner.(*composedRunner).auth.(*ProcessRunner).threadEffort {
		t.Fatal("AppServerThreadEffort=false was ignored")
	}
	if _, err := NewRunner(RunnerOptions{Transport: "grpc"}); err == nil {
		t.Fatal("an unknown transport must fail")
	}
	if _, err := NewRunner(RunnerOptions{Transport: TransportDirect, Direct: DirectConfig{RequestShape: "huge"}}); err == nil {
		t.Fatal("an unknown request shape must fail")
	}
	if _, err := NewRunner(RunnerOptions{Transport: TransportDirect, Direct: DirectConfig{MaxRetries: 4}}); err == nil {
		t.Fatal("more than 3 retries must fail")
	}
}

// inferenceCoreBrokerPhases mirrors BROKER_PHASES in Model Plane's
// inference-core (src/provider/codex_subscription.rs, lines 28-35): the only
// phasesMs keys it forwards. Every key either transport emits must be here.
var inferenceCoreBrokerPhases = []string{"spawn", "initialize", "thread_start", "turn_start", "first_delta", "turn_total"}

func TestPhaseKeysSubsetOfInferenceCore(t *testing.T) {
	for _, phase := range []string{PhaseSpawn, PhaseInitialize, PhaseThreadStart, PhaseTurnStart, PhaseFirstDelta, PhaseTurnTotal} {
		if !slices.Contains(inferenceCoreBrokerPhases, phase) {
			t.Fatalf("phase %q is not forwarded by inference-core", phase)
		}
	}
	h := newDirectHarness(t, nil)
	freshAuth(t, h.home, h.now)
	direct, err := h.transport.Invoke(t.Context(), h.home, testRequest())
	if err != nil {
		t.Fatal(err)
	}
	for phase := range direct.PhasesMs {
		if !slices.Contains(inferenceCoreBrokerPhases, phase) {
			t.Fatalf("direct transport emitted phase %q", phase)
		}
	}

	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	t.Setenv(fakeAppServerEnv, "1")
	home := t.TempDir()
	if err := os.WriteFile(home+"/auth.json", []byte("{}"), 0o600); err != nil {
		t.Fatal(err)
	}
	appServer, err := NewProcessRunner(executable).Invoke(t.Context(), home, testRequest())
	if err != nil {
		t.Fatal(err)
	}
	for phase := range appServer.PhasesMs {
		if !slices.Contains(inferenceCoreBrokerPhases, phase) {
			t.Fatalf("app-server transport emitted phase %q", phase)
		}
	}
}
