package main

import (
	"errors"
	"testing"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/threadworkspace"
)

// storeEnv is an environment with an object store configured, plus extra. The
// credentials are low-entropy fixtures: nothing here reaches a server.
func storeEnv(extra map[string]string) func(string) string {
	env := map[string]string{
		"MINIO_ENDPOINT":   "http://minio:9000",
		"MINIO_ACCESS_KEY": "fixture-access-aaaaaaaaaaaaaaaaaaaa",
		"MINIO_SECRET_KEY": "fixture-secret-aaaaaaaaaaaaaaaaaaaa",
	}
	for name, value := range extra {
		env[name] = value
	}
	return func(name string) string { return env[name] }
}

// TestThreadErasureBuildsTheClientFromAConfiguredStore: a configured store gives the
// client and the default retention. The helper takes no switch, so the same build
// serves the switch off and on (F-f06-02).
func TestThreadErasureBuildsTheClientFromAConfiguredStore(t *testing.T) {
	blobs, retention, err := threadErasureFromEnv(storeEnv(nil))
	if err != nil || blobs == nil {
		t.Fatalf("configured store: client = %v, err = %v; want a client", blobs, err)
	}
	if retention != threadworkspace.DefaultRetention {
		t.Fatalf("retention = %s, want the default", retention)
	}
}

// TestThreadErasureWithoutAUsableStoreIsErrObjectStore: an unset or malformed store
// is errObjectStore with no client, so the caller can warn (switch off) or refuse
// to start (switch on).
func TestThreadErasureWithoutAUsableStoreIsErrObjectStore(t *testing.T) {
	cases := map[string]func(string) string{
		"unset":     func(string) string { return "" },
		"malformed": storeEnv(map[string]string{"MINIO_ENDPOINT": "ftp://minio:9000"}),
	}
	for name, getenv := range cases {
		blobs, _, err := threadErasureFromEnv(getenv)
		if !errors.Is(err, errObjectStore) || blobs != nil {
			t.Fatalf("%s: client = %v, err = %v; want errObjectStore and no client", name, blobs, err)
		}
	}
}

// TestThreadErasureRetentionErrorIsNotAMissingStore: a retention that does not parse
// stops startup whichever way the switch is set, so it must not be taken for a
// missing store that a switch-off deployment may tolerate.
func TestThreadErasureRetentionErrorIsNotAMissingStore(t *testing.T) {
	_, _, err := threadErasureFromEnv(storeEnv(map[string]string{"SANDBOX_MANAGER_THREAD_RETENTION_DAYS": "soon"}))
	if err == nil || errors.Is(err, errObjectStore) {
		t.Fatalf("a bad retention: err = %v; want a retention error, not a missing store", err)
	}
}
