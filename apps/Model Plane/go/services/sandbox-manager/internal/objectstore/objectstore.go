// Package objectstore erases the blobs one thread's workspace wrote to the
// content-addressed store (WS22-S5, DEC-23). It talks to the MinIO bucket that
// the execution-core CAS client writes (workspace_cas.rs), under the same
// environment names, and it can do exactly one thing: remove every object under
// one thread's prefix. It has no read path and no write path for content, so
// the sandbox-manager gains no authority over a thread's bytes beyond erasing
// them.
package objectstore

import (
	"context"
	"errors"
	"fmt"
	"net/url"
	"strings"

	"github.com/minio/minio-go/v7"
	"github.com/minio/minio-go/v7/pkg/credentials"
)

// The bucket and region defaults match workspace_cas.rs, so erasure reaches the
// bucket the CAS writes to.
const (
	DefaultBucket = "model-plane-artifacts"
	DefaultRegion = "us-east-1"
)

// ErrInvalidScope refuses an org or thread id that is not a plain key segment,
// the rule workspace_cas.rs applies. Without it a crafted id could reach
// another thread's prefix.
var ErrInvalidScope = errors.New("thread workspace ids must be plain key segments")

// Config is one MinIO connection. The environment names it is read from are the
// ones workspace_cas.rs reads.
type Config struct {
	Endpoint  string
	AccessKey string
	SecretKey string
	Bucket    string
	Region    string
}

// ConfigFromEnv reads MINIO_ENDPOINT, MINIO_ACCESS_KEY, MINIO_SECRET_KEY,
// OBJECT_STORAGE_BUCKET and MINIO_REGION. An absent endpoint, access key or
// secret key is a disabled client, not an error, which is how CasClient::from_env
// treats the same absence. A present but malformed endpoint is an error. No
// error ever carries the secret.
func ConfigFromEnv(getenv func(string) string) (Config, bool, error) {
	endpoint := strings.TrimSpace(getenv("MINIO_ENDPOINT"))
	accessKey := strings.TrimSpace(getenv("MINIO_ACCESS_KEY"))
	secretKey := strings.TrimSpace(getenv("MINIO_SECRET_KEY"))
	if endpoint == "" || accessKey == "" || secretKey == "" {
		return Config{}, false, nil
	}
	cfg := Config{
		Endpoint:  endpoint,
		AccessKey: accessKey,
		SecretKey: secretKey,
		Bucket:    DefaultBucket,
		Region:    DefaultRegion,
	}
	if bucket := strings.TrimSpace(getenv("OBJECT_STORAGE_BUCKET")); bucket != "" {
		cfg.Bucket = bucket
	}
	if region := strings.TrimSpace(getenv("MINIO_REGION")); region != "" {
		cfg.Region = region
	}
	if _, _, err := parseEndpoint(cfg.Endpoint); err != nil {
		return Config{}, false, err
	}
	return cfg, true, nil
}

// parseEndpoint accepts an http or https origin with a host and no path, and
// returns the host:port that minio-go wants with the TLS choice.
func parseEndpoint(endpoint string) (host string, secure bool, err error) {
	u, err := url.Parse(endpoint)
	if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Host == "" || (u.Path != "" && u.Path != "/") {
		return "", false, errors.New("MINIO_ENDPOINT must be an http or https origin")
	}
	return u.Host, u.Scheme == "https", nil
}

// objectAPI is the part of *minio.Client this package uses. It is an interface
// so the removal logic is tested without a server.
type objectAPI interface {
	ListObjects(ctx context.Context, bucketName string, opts minio.ListObjectsOptions) <-chan minio.ObjectInfo
	RemoveObjects(ctx context.Context, bucketName string, objectsCh <-chan minio.ObjectInfo, opts minio.RemoveObjectsOptions) <-chan minio.RemoveObjectError
}

// Client removes thread prefixes from one bucket.
type Client struct {
	api    objectAPI
	bucket string
}

// New builds a client for cfg. It does not contact the store.
func New(cfg Config) (*Client, error) {
	host, secure, err := parseEndpoint(cfg.Endpoint)
	if err != nil {
		return nil, err
	}
	if cfg.Bucket == "" {
		return nil, errors.New("object store bucket is required")
	}
	api, err := minio.New(host, &minio.Options{
		Creds:  credentials.NewStaticV4(cfg.AccessKey, cfg.SecretKey, ""),
		Secure: secure,
		Region: cfg.Region,
	})
	if err != nil {
		return nil, fmt.Errorf("object store client: %w", err)
	}
	return &Client{api: api, bucket: cfg.Bucket}, nil
}

// ThreadPrefix is the key prefix under which one thread's blobs live. It is
// the scope thread_scope builds in workspace_cas.rs, under the cas/ root that
// put_scoped adds, so the prefix holds that thread's blobs and no other
// thread's. The trailing slash keeps thread "a" from matching thread "ab".
func ThreadPrefix(orgID, threadID string) (string, error) {
	if !isKeySegment(orgID) || !isKeySegment(threadID) {
		return "", ErrInvalidScope
	}
	return "cas/thread/" + orgID + "/" + threadID + "/", nil
}

// DeleteThreadBlobs removes every blob under the thread's prefix and reports how
// many it removed. It is safe to call again after a partial failure, because
// objects already gone are not listed.
func (c *Client) DeleteThreadBlobs(ctx context.Context, orgID, threadID string) (int64, error) {
	prefix, err := ThreadPrefix(orgID, threadID)
	if err != nil {
		return 0, err
	}
	return c.deleteUnder(ctx, prefix)
}

// deleteUnder lists the prefix and streams what it lists into RemoveObjects. The
// forwarding goroutine owns the counter and the listing error, and publishes
// both by closing forwarded, so they are read only after that close.
func (c *Client) deleteUnder(ctx context.Context, prefix string) (int64, error) {
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()

	objects := make(chan minio.ObjectInfo)
	forwarded := make(chan struct{})
	var listed int64
	var listErr error
	go func() {
		defer close(forwarded)
		defer close(objects)
		for obj := range c.api.ListObjects(ctx, c.bucket, minio.ListObjectsOptions{Prefix: prefix, Recursive: true}) {
			if obj.Err != nil {
				listErr = obj.Err
				return
			}
			select {
			case objects <- obj:
				listed++
			case <-ctx.Done():
				return
			}
		}
	}()

	// RemoveObjects reports only the objects it failed to remove, so every
	// report is a failure and the first one is kept.
	var removeErr error
	for failure := range c.api.RemoveObjects(ctx, c.bucket, objects, minio.RemoveObjectsOptions{}) {
		if removeErr == nil {
			removeErr = fmt.Errorf("remove %q: %w", failure.ObjectName, failure.Err)
		}
	}
	cancel()
	<-forwarded

	if listErr != nil {
		return listed, fmt.Errorf("list thread blobs: %w", listErr)
	}
	if removeErr != nil {
		return listed, removeErr
	}
	return listed, nil
}

// isKeySegment mirrors workspace_cas.rs: one to 128 ASCII letters, digits,
// underscores or hyphens.
func isKeySegment(segment string) bool {
	if len(segment) < 1 || len(segment) > 128 {
		return false
	}
	for i := 0; i < len(segment); i++ {
		b := segment[i]
		if !(b >= 'a' && b <= 'z' || b >= 'A' && b <= 'Z' || b >= '0' && b <= '9' || b == '_' || b == '-') {
			return false
		}
	}
	return true
}
