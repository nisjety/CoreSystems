package objectstore

import (
	"bytes"
	"context"
	"fmt"
	"net/url"
	"path"
	"strings"
	"time"

	"github.com/minio/minio-go/v7"
	"github.com/minio/minio-go/v7/pkg/credentials"
)

type MinIOConfig struct {
	Endpoint      string
	AccessKey     string
	SecretKey     string
	Bucket        string
	UseSSL        bool
	Region        string
	PublicBaseURL string
}

type MinIOStore struct {
	client        *minio.Client
	bucket        string
	region        string
	publicBaseURL string
}

func NewMinIOStore(cfg MinIOConfig) (*MinIOStore, error) {
	endpoint, useSSL, err := normalizeEndpoint(cfg.Endpoint, cfg.UseSSL)
	if err != nil {
		return nil, err
	}
	if strings.TrimSpace(cfg.AccessKey) == "" || strings.TrimSpace(cfg.SecretKey) == "" {
		return nil, fmt.Errorf("minio credentials are required")
	}
	if strings.TrimSpace(cfg.Bucket) == "" {
		return nil, fmt.Errorf("minio bucket is required")
	}

	client, err := minio.New(endpoint, &minio.Options{
		Creds:  credentials.NewStaticV4(cfg.AccessKey, cfg.SecretKey, ""),
		Secure: useSSL,
		Region: strings.TrimSpace(cfg.Region),
	})
	if err != nil {
		return nil, fmt.Errorf("initialize minio client: %w", err)
	}

	store := &MinIOStore{
		client:        client,
		bucket:        strings.TrimSpace(cfg.Bucket),
		region:        strings.TrimSpace(cfg.Region),
		publicBaseURL: strings.TrimRight(strings.TrimSpace(cfg.PublicBaseURL), "/"),
	}

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	exists, err := client.BucketExists(ctx, store.bucket)
	if err != nil {
		return nil, fmt.Errorf("check minio bucket %s: %w", store.bucket, err)
	}
	if !exists {
		if err := client.MakeBucket(ctx, store.bucket, minio.MakeBucketOptions{Region: store.region}); err != nil {
			return nil, fmt.Errorf("create minio bucket %s: %w", store.bucket, err)
		}
	}

	return store, nil
}

func (s *MinIOStore) Enabled() bool {
	return s != nil && s.client != nil
}

func (s *MinIOStore) PutBytes(ctx context.Context, key string, data []byte, opts PutOptions) (*Artifact, error) {
	if !s.Enabled() {
		return nil, fmt.Errorf("minio store is not initialized")
	}
	if strings.TrimSpace(key) == "" {
		return nil, fmt.Errorf("artifact key is required")
	}

	info, err := s.client.PutObject(ctx, s.bucket, strings.TrimLeft(key, "/"), bytes.NewReader(data), int64(len(data)), minio.PutObjectOptions{
		ContentType:  strings.TrimSpace(opts.ContentType),
		UserMetadata: cloneMetadata(opts.Metadata),
	})
	if err != nil {
		return nil, fmt.Errorf("put object %s: %w", key, err)
	}

	return &Artifact{
		Kind:        opts.Kind,
		Provider:    "minio",
		Bucket:      s.bucket,
		Key:         strings.TrimLeft(key, "/"),
		URL:         s.objectURL(strings.TrimLeft(key, "/")),
		ContentType: strings.TrimSpace(opts.ContentType),
		Size:        info.Size,
		ETag:        strings.Trim(info.ETag, `"`),
	}, nil
}

func (s *MinIOStore) Close() error {
	return nil
}

func (s *MinIOStore) objectURL(key string) string {
	if s.publicBaseURL == "" {
		return ""
	}
	return s.publicBaseURL + "/" + s.bucket + "/" + escapeObjectKey(key)
}

func cloneMetadata(metadata map[string]string) map[string]string {
	if len(metadata) == 0 {
		return nil
	}
	out := make(map[string]string, len(metadata))
	for key, value := range metadata {
		out[key] = value
	}
	return out
}

func normalizeEndpoint(raw string, useSSL bool) (string, bool, error) {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" {
		return "", false, fmt.Errorf("minio endpoint is required")
	}

	if strings.HasPrefix(trimmed, "http://") || strings.HasPrefix(trimmed, "https://") {
		parsed, err := url.Parse(trimmed)
		if err != nil {
			return "", false, fmt.Errorf("parse minio endpoint: %w", err)
		}
		return parsed.Host, parsed.Scheme == "https", nil
	}

	return trimmed, useSSL, nil
}

func escapeObjectKey(key string) string {
	segments := strings.Split(strings.TrimLeft(key, "/"), "/")
	for index, segment := range segments {
		segments[index] = url.PathEscape(segment)
	}
	return path.Join(segments...)
}
