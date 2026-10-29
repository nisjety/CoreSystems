package main

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"flag"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"sync/atomic"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/bearerrefresh"
)

func TestStudioBundleUsesFixedProfilesAndPaths(t *testing.T) {
	parent := t.TempDir()
	directory := filepath.Join(parent, "private-bundle")
	base := bearerrefresh.Config{ServiceID: "social-core", OrganizationID: "test-org", TokenFile: "/ignored/arbitrary", Capability: "ignored"}
	configs, err := studioBundleConfigs(base, directory, true)
	if err != nil {
		t.Fatal(err)
	}
	if len(configs) != 4 {
		t.Fatal("expected exactly four audience-separated files")
	}
	names := []string{"integration", "quarry", "model", "inference"}
	profiles := []string{"integration-readwrite", "quarry-capture", "model-draft", "inference-draft"}
	for index, cfg := range configs {
		if cfg.TokenFile != filepath.Join(directory, names[index]) || cfg.Capability != profiles[index] || cfg.ServiceID != base.ServiceID || cfg.OrganizationID != base.OrganizationID {
			t.Fatal("bundle policy was not fixed")
		}
	}
	info, _ := os.Lstat(directory)
	if info.Mode().Perm() != 0700 {
		t.Fatal("bundle directory is not private")
	}
	if base.TokenFile != "/ignored/arbitrary" || base.Capability != "ignored" {
		t.Fatal("caller config mutated")
	}
	if _, err := studioBundleConfigs(base, filepath.Join(parent, "absent-check"), false); err == nil {
		t.Fatal("health check created missing directory")
	}
}

func TestStudioBundleRejectsUnsafeDirectoriesAndMissingIdentity(t *testing.T) {
	base := bearerrefresh.Config{ServiceID: "leads-core"}
	parent := t.TempDir()
	public := filepath.Join(parent, "public")
	if err := os.Mkdir(public, 0755); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(parent, "link")
	if err := os.Symlink(public, link); err != nil {
		t.Fatal(err)
	}
	for _, directory := range []string{"", ".", "/", "relative/path", parent + "/../bad", public, link, filepath.Join(parent, "absent", "child")} {
		if _, err := studioBundleConfigs(base, directory, true); err == nil {
			t.Fatalf("unsafe directory accepted: %s", directory)
		}
	}
	base.ServiceID = ""
	if _, err := studioBundleConfigs(base, filepath.Join(parent, "private"), true); err == nil {
		t.Fatal("bundle requires explicit service identity")
	}
}

type bundleStub struct {
	refresh, check, run atomic.Int32
	fail                bool
	stopped             chan struct{}
}

func (stub *bundleStub) Refresh(context.Context) (bearerrefresh.State, error) {
	stub.refresh.Add(1)
	return bearerrefresh.State{}, nil
}
func (stub *bundleStub) Check() (bearerrefresh.State, error) {
	stub.check.Add(1)
	if stub.fail {
		return bearerrefresh.State{}, errors.New("synthetic private failure")
	}
	return bearerrefresh.State{}, nil
}
func (stub *bundleStub) Run(ctx context.Context, report func(bearerrefresh.State, error)) error {
	stub.run.Add(1)
	if report != nil {
		report(bearerrefresh.State{}, nil)
	}
	if stub.fail {
		return errors.New("synthetic private failure")
	}
	<-ctx.Done()
	if stub.stopped != nil {
		close(stub.stopped)
	}
	return ctx.Err()
}

func TestStudioBundleOnceAndCheckNeverStartRenewalLoops(t *testing.T) {
	for _, check := range []bool{false, true} {
		stubs := []*bundleStub{{}, {}, {}, {}}
		workers := []bundleWorker{{"integration", stubs[0]}, {"quarry", stubs[1]}, {"model", stubs[2]}, {"inference", stubs[3]}}
		if err := runStudioBundle(context.Background(), workers, !check, check, nil); err != nil {
			t.Fatal(err)
		}
		for _, stub := range stubs {
			if stub.run.Load() != 0 || (check && (stub.refresh.Load() != 0 || stub.check.Load() != 1)) || (!check && (stub.check.Load() != 0 || stub.refresh.Load() != 1)) {
				t.Fatal("wrong bundle operation ran")
			}
		}
	}
	stub := &bundleStub{fail: true}
	err := runStudioBundle(context.Background(), []bundleWorker{{"integration", stub}}, false, true, nil)
	if err == nil || reflect.DeepEqual(err.Error(), "synthetic private failure") {
		t.Fatal("failure absent or leaked internal details")
	}
}

func TestStudioBundleHealthChecksFourSyntheticAudienceFilesWithoutIssuance(t *testing.T) {
	var requests atomic.Int32
	server := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests.Add(1)
		w.WriteHeader(http.StatusServiceUnavailable)
	}))
	defer server.Close()
	parent := t.TempDir()
	if err := os.Chmod(parent, 0700); err != nil {
		t.Fatal(err)
	}
	key := filepath.Join(parent, "service-key")
	if err := os.WriteFile(key, []byte("synthetic-bundle-service-key"), 0600); err != nil {
		t.Fatal(err)
	}
	base := bearerrefresh.Config{AuthCoreURL: server.URL, ServiceID: "social-core", OrganizationID: "test-org", ServiceKeyFile: key}
	configs, err := studioBundleConfigs(base, filepath.Join(parent, "bundle"), true)
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now().UTC()
	audiences := []string{"ingestion", "quarry", "model-gateway", "inference-core"}
	scopes := [][]string{{"integration:read", "integration:write"}, {"map:read", "scrape:read", "scrape:write"}, {"models:invoke"}, {"inference:invoke"}}
	for index, cfg := range configs {
		claims, _ := json.Marshal(map[string]any{"iss": "http://localhost:3011/api/convex-auth", "aud": audiences[index], "sub": "service:social-core", "service_id": "service:social-core", "principal_type": "service", "org_id": "test-org", "scopes": scopes[index], "iat": now.Unix(), "nbf": now.Add(-5 * time.Second).Unix(), "exp": now.Add(5 * time.Minute).Unix()})
		header := base64.RawURLEncoding.EncodeToString([]byte(`{"alg":"RS256","kid":"synthetic-only"}`))
		token := header + "." + base64.RawURLEncoding.EncodeToString(claims) + ".synthetic-signature"
		if err := os.WriteFile(cfg.TokenFile, []byte(token), 0600); err != nil {
			t.Fatal(err)
		}
	}
	workers, err := newBundleWorkers(configs, server.Client(), func() time.Time { return now })
	if err != nil {
		t.Fatal(err)
	}
	if err := runStudioBundle(context.Background(), workers, false, true, nil); err != nil {
		t.Fatal(err)
	}
	if requests.Load() != 0 {
		t.Fatal("health check issued an Auth request")
	}
	// Exercise the existing binary's real flag path without minting a token.
	t.Setenv("AUTH_CORE_URL", server.URL)
	t.Setenv("STUDIO_SERVICE_ID", "social-core")
	t.Setenv("STUDIO_ORGANIZATION_ID", "test-org")
	t.Setenv("STUDIO_SERVICE_KEY_FILE", key)
	t.Setenv("STUDIO_BEARER_DIRECTORY", filepath.Join(parent, "bundle"))
	originalFlags, originalArgs := flag.CommandLine, os.Args
	t.Cleanup(func() { flag.CommandLine, os.Args = originalFlags, originalArgs })
	flag.CommandLine = flag.NewFlagSet("integration-bearer-refresh", flag.ContinueOnError)
	os.Args = []string{"integration-bearer-refresh", "--studio-bundle", "--check"}
	main()
	if err := os.WriteFile(configs[2].TokenFile, []byte("invalid"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := runStudioBundle(context.Background(), workers, false, true, nil); err == nil {
		t.Fatal("bundle with one unready audience passed health check")
	}
	if requests.Load() != 0 {
		t.Fatal("failed health check issued an Auth request")
	}
}

func TestStudioBundleCancelsAndDrainsSiblingRenewersOnFailure(t *testing.T) {
	stopped := make(chan struct{})
	failing, sibling := &bundleStub{fail: true}, &bundleStub{stopped: stopped}
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	var reports atomic.Int32
	if err := runStudioBundle(ctx, []bundleWorker{{"integration", failing}, {"quarry", sibling}}, false, false, func(string, bearerrefresh.State, error) { reports.Add(1) }); err == nil {
		t.Fatal("failed group returned success")
	}
	select {
	case <-stopped:
	default:
		t.Fatal("sibling was not stopped before group returned")
	}
	if failing.run.Load() != 1 || sibling.run.Load() != 1 {
		t.Fatal("not all workers ran")
	}
	if reports.Load() != 2 {
		t.Fatal("worker reporting did not preserve both fixed profile names")
	}
}

func TestStudioBundleInvalidOperationsAndCancellation(t *testing.T) {
	stub := &bundleStub{}
	workers := []bundleWorker{{"integration", stub}}
	if runStudioBundle(context.Background(), nil, false, false, nil) == nil || runStudioBundle(context.Background(), workers, true, true, nil) == nil {
		t.Fatal("invalid bundle operation accepted")
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	for _, once := range []bool{true, false} {
		if err := runStudioBundle(ctx, workers, once, false, nil); !errors.Is(err, context.Canceled) {
			t.Fatal("cancellation was not preserved")
		}
	}
	if _, err := newBundleWorkers([]bearerrefresh.Config{{ServiceID: "invalid/service"}}, nil, nil); err == nil {
		t.Fatal("invalid startup principal accepted")
	}
}
