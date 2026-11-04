package objectstore

import (
	"context"
	"errors"
	"sort"
	"strings"
	"sync"
	"testing"

	"github.com/minio/minio-go/v7"
)

// fakeAPI is an in-memory bucket with the two calls the client makes. Its map is
// touched only under mu, because removal runs while listing is still streaming.
type fakeAPI struct {
	mu         sync.Mutex
	objects    map[string]bool
	listErr    error
	removeFail string
	listCalls  int
}

func (f *fakeAPI) ListObjects(ctx context.Context, _ string, opts minio.ListObjectsOptions) <-chan minio.ObjectInfo {
	f.mu.Lock()
	f.listCalls++
	var keys []string
	for key := range f.objects {
		if strings.HasPrefix(key, opts.Prefix) {
			keys = append(keys, key)
		}
	}
	listErr := f.listErr
	f.mu.Unlock()
	sort.Strings(keys)

	out := make(chan minio.ObjectInfo)
	go func() {
		defer close(out)
		for _, key := range keys {
			select {
			case out <- minio.ObjectInfo{Key: key}:
			case <-ctx.Done():
				return
			}
		}
		if listErr != nil {
			select {
			case out <- minio.ObjectInfo{Err: listErr}:
			case <-ctx.Done():
			}
		}
	}()
	return out
}

func (f *fakeAPI) RemoveObjects(_ context.Context, _ string, objectsCh <-chan minio.ObjectInfo, _ minio.RemoveObjectsOptions) <-chan minio.RemoveObjectError {
	failures := make(chan minio.RemoveObjectError)
	go func() {
		defer close(failures)
		for obj := range objectsCh {
			f.mu.Lock()
			if obj.Key == f.removeFail {
				f.mu.Unlock()
				failures <- minio.RemoveObjectError{ObjectName: obj.Key, Err: errors.New("access denied")}
				continue
			}
			delete(f.objects, obj.Key)
			f.mu.Unlock()
		}
	}()
	return failures
}

func (f *fakeAPI) remaining() []string {
	f.mu.Lock()
	defer f.mu.Unlock()
	var keys []string
	for key := range f.objects {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	return keys
}

func TestThreadPrefixIsTheScopeTheCasWritesUnder(t *testing.T) {
	t.Parallel()
	got, err := ThreadPrefix("org_1", "thr_1")
	if err != nil {
		t.Fatalf("ThreadPrefix: %v", err)
	}
	if want := "cas/thread/org_1/thr_1/"; got != want {
		t.Fatalf("prefix = %q, want %q", got, want)
	}
}

// TestThreadPrefixRefusesIdsThatCouldReachAnotherThread: an id that is not a plain
// key segment could walk out of its own prefix, so it is refused before anything
// is listed.
func TestThreadPrefixRefusesIdsThatCouldReachAnotherThread(t *testing.T) {
	t.Parallel()
	long := strings.Repeat("a", 129)
	cases := []struct{ org, thread string }{
		{"", "thr_1"},
		{"org_1", ""},
		{"org/1", "thr_1"},
		{"org_1", "../thr_2"},
		{"org_1", "thr.1"},
		{"org 1", "thr_1"},
		{"org_1", "thr\x001"},
		{"org_1", long},
	}
	for _, c := range cases {
		if _, err := ThreadPrefix(c.org, c.thread); !errors.Is(err, ErrInvalidScope) {
			t.Fatalf("ThreadPrefix(%q, %q) err = %v, want ErrInvalidScope", c.org, c.thread, err)
		}
	}
}

// TestDeleteThreadBlobsRemovesOnlyTheThreadsOwnPrefix: the sibling thread whose id
// extends this one, and the same thread id under another org, both keep their blobs.
func TestDeleteThreadBlobsRemovesOnlyTheThreadsOwnPrefix(t *testing.T) {
	t.Parallel()
	api := &fakeAPI{objects: map[string]bool{
		"cas/thread/org_1/thr_1/sha256a":  true,
		"cas/thread/org_1/thr_1/sha256b":  true,
		"cas/thread/org_1/thr_10/sha256c": true,
		"cas/thread/org_2/thr_1/sha256d":  true,
		"cas/sha256e":                     true,
	}}
	client := &Client{api: api, bucket: "bucket"}

	removed, err := client.DeleteThreadBlobs(context.Background(), "org_1", "thr_1")
	if err != nil {
		t.Fatalf("DeleteThreadBlobs: %v", err)
	}
	if removed != 2 {
		t.Fatalf("removed = %d, want 2", removed)
	}
	want := []string{"cas/sha256e", "cas/thread/org_1/thr_10/sha256c", "cas/thread/org_2/thr_1/sha256d"}
	if got := api.remaining(); strings.Join(got, "|") != strings.Join(want, "|") {
		t.Fatalf("remaining = %v, want %v", got, want)
	}
}

func TestDeleteThreadBlobsReportsAListingFailure(t *testing.T) {
	t.Parallel()
	listErr := errors.New("connection reset")
	api := &fakeAPI{objects: map[string]bool{"cas/thread/org_1/thr_1/sha256a": true}, listErr: listErr}
	client := &Client{api: api, bucket: "bucket"}

	if _, err := client.DeleteThreadBlobs(context.Background(), "org_1", "thr_1"); !errors.Is(err, listErr) {
		t.Fatalf("err = %v, want the listing failure", err)
	}
}

func TestDeleteThreadBlobsReportsARemovalFailureAndNamesTheObject(t *testing.T) {
	t.Parallel()
	api := &fakeAPI{
		objects: map[string]bool{
			"cas/thread/org_1/thr_1/sha256a": true,
			"cas/thread/org_1/thr_1/sha256b": true,
		},
		removeFail: "cas/thread/org_1/thr_1/sha256a",
	}
	client := &Client{api: api, bucket: "bucket"}

	_, err := client.DeleteThreadBlobs(context.Background(), "org_1", "thr_1")
	if err == nil || !strings.Contains(err.Error(), "sha256a") {
		t.Fatalf("err = %v, want a removal failure naming the object", err)
	}
}

func TestDeleteThreadBlobsRefusesAnInvalidScopeWithoutListingAnything(t *testing.T) {
	t.Parallel()
	api := &fakeAPI{objects: map[string]bool{"cas/thread/org_1/thr_1/sha256a": true}}
	client := &Client{api: api, bucket: "bucket"}

	if _, err := client.DeleteThreadBlobs(context.Background(), "org_1", "../thr_1"); !errors.Is(err, ErrInvalidScope) {
		t.Fatalf("err = %v, want ErrInvalidScope", err)
	}
	if api.listCalls != 0 {
		t.Fatalf("listed the bucket %d times for an invalid scope", api.listCalls)
	}
}

func TestConfigFromEnvIsDisabledWithoutEndpointOrCredentials(t *testing.T) {
	t.Parallel()
	for _, env := range []map[string]string{
		{},
		{"MINIO_ACCESS_KEY": "ak", "MINIO_SECRET_KEY": "sk"},
		{"MINIO_ENDPOINT": "http://minio:9000", "MINIO_ACCESS_KEY": "ak"},
		{"MINIO_ENDPOINT": "http://minio:9000", "MINIO_SECRET_KEY": "sk"},
	} {
		_, ok, err := ConfigFromEnv(func(key string) string { return env[key] })
		if ok || err != nil {
			t.Fatalf("env %v: ok = %v, err = %v; want a disabled client and no error", env, ok, err)
		}
	}
}

func TestConfigFromEnvReadsTheCasNamesWithTheCasDefaults(t *testing.T) {
	t.Parallel()
	env := map[string]string{"MINIO_ENDPOINT": "http://minio:9000", "MINIO_ACCESS_KEY": "ak", "MINIO_SECRET_KEY": "sk"}
	cfg, ok, err := ConfigFromEnv(func(key string) string { return env[key] })
	if err != nil || !ok {
		t.Fatalf("ok = %v, err = %v", ok, err)
	}
	if cfg.Bucket != DefaultBucket || cfg.Region != DefaultRegion {
		t.Fatalf("bucket = %q, region = %q; want the CAS defaults", cfg.Bucket, cfg.Region)
	}

	env["OBJECT_STORAGE_BUCKET"] = "artifacts-eu"
	env["MINIO_REGION"] = "eu-west-1"
	cfg, _, _ = ConfigFromEnv(func(key string) string { return env[key] })
	if cfg.Bucket != "artifacts-eu" || cfg.Region != "eu-west-1" {
		t.Fatalf("bucket = %q, region = %q; want the overrides", cfg.Bucket, cfg.Region)
	}
}

// TestConfigFromEnvRefusesAMalformedEndpointWithoutEchoingTheSecret: a present but
// malformed endpoint is an error, and the error never carries the secret.
func TestConfigFromEnvRefusesAMalformedEndpointWithoutEchoingTheSecret(t *testing.T) {
	t.Parallel()
	const secret = "not-a-real-secret-value"
	for _, endpoint := range []string{"minio:9000", "ftp://minio:9000", "http://minio:9000/extra", "http://"} {
		env := map[string]string{"MINIO_ENDPOINT": endpoint, "MINIO_ACCESS_KEY": "ak", "MINIO_SECRET_KEY": secret}
		_, ok, err := ConfigFromEnv(func(key string) string { return env[key] })
		if err == nil || ok {
			t.Fatalf("endpoint %q: ok = %v, err = %v; want a refusal", endpoint, ok, err)
		}
		if strings.Contains(err.Error(), secret) {
			t.Fatalf("endpoint %q: the error echoes the secret", endpoint)
		}
	}
}

func TestNewRequiresABucket(t *testing.T) {
	t.Parallel()
	cfg := Config{Endpoint: "https://minio.internal:9000", AccessKey: "ak", SecretKey: "sk", Region: DefaultRegion}
	if _, err := New(cfg); err == nil {
		t.Fatal("New accepted a config without a bucket")
	}
	cfg.Bucket = DefaultBucket
	if _, err := New(cfg); err != nil {
		t.Fatalf("New: %v", err)
	}
}
