package social

import (
	"bytes"
	"context"
	"crypto/aes"
	"crypto/cipher"
	"encoding/base64"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
)

type fakePublishActionExecutor struct {
	calls  []ActionRequest
	result *ActionResult
	err    error
}

func (f *fakePublishActionExecutor) ExecuteAction(_ context.Context, request ActionRequest) (*ActionResult, error) {
	f.calls = append(f.calls, request)
	return f.result, f.err
}

func TestGovernedPublisherRoutesLinkedInPublishThroughActionExecutor(t *testing.T) {
	executor := &fakePublishActionExecutor{result: &ActionResult{
		ProviderKey: "linkedin",
		Operation:   "linkedin.posts.create",
		Result:      mustJSONRaw(t, map[string]any{"id": "urn:li:share:123"}),
	}}
	publisher := NewGovernedPublisher(executor)

	attempt := publisher.Publish(context.Background(), PublishJob{ID: "job_1", OrgID: "org_1", PostID: "post_1"}, Post{
		ID:        "post_1",
		OrgID:     "org_1",
		Body:      "A governed announcement",
		Platforms: []string{"linkedin"},
	}, Account{
		OrgID:        "org_1",
		ProviderKey:  "linkedin",
		ConnectionID: "conn_1",
		Status:       AccountStatusConnected,
		Capabilities: []string{"social.post.write"},
		Metadata:     map[string]any{"author_urn": "urn:li:organization:42"},
	})

	if attempt.Status != AttemptStatusSucceeded {
		t.Fatalf("attempt status = %s, want succeeded: %#v", attempt.Status, attempt)
	}
	if attempt.ExternalID != "urn:li:share:123" {
		t.Fatalf("external id = %q", attempt.ExternalID)
	}
	if len(executor.calls) != 1 {
		t.Fatalf("action calls = %d, want 1", len(executor.calls))
	}
	call := executor.calls[0]
	if call.ConnectionID != "conn_1" || call.Operation != "linkedin.posts.create" {
		t.Fatalf("action call = %#v", call)
	}
	if call.Body["author"] != "urn:li:organization:42" || call.Body["commentary"] != "A governed announcement" {
		t.Fatalf("action body = %#v", call.Body)
	}
}

func TestGovernedPublisherBlocksProviderWithoutActionContract(t *testing.T) {
	executor := &fakePublishActionExecutor{}
	publisher := NewGovernedPublisher(executor)

	attempt := publisher.Publish(context.Background(), PublishJob{ID: "job_1", OrgID: "org_1", PostID: "post_1"}, Post{
		ID:        "post_1",
		OrgID:     "org_1",
		Body:      "Never lease a raw token",
		Platforms: []string{"x"},
	}, Account{
		OrgID:        "org_1",
		ProviderKey:  "x",
		ConnectionID: "conn_1",
		Status:       AccountStatusConnected,
		Capabilities: []string{"social.post.write"},
	})

	if attempt.Status != AttemptStatusBlocked {
		t.Fatalf("attempt status = %s, want blocked", attempt.Status)
	}
	if !strings.Contains(attempt.Message, "governed integration action") {
		t.Fatalf("attempt message = %q", attempt.Message)
	}
	if len(executor.calls) != 0 {
		t.Fatalf("unsupported provider must not execute an action: %#v", executor.calls)
	}
}

func TestGovernedPublisherRoutesMetaPublishSequenceThroughActionExecutor(t *testing.T) {
	executor := &fakePublishActionExecutor{result: &ActionResult{
		ProviderKey: "meta",
		Result:      mustJSONRaw(t, map[string]any{"id": "provider-id"}),
	}}
	publisher := NewGovernedPublisher(executor)

	attempt := publisher.Publish(context.Background(), PublishJob{ID: "job_1", OrgID: "org_1", PostID: "post_1"}, Post{
		ID:        "post_1",
		OrgID:     "org_1",
		Body:      "A governed visual announcement",
		Platforms: []string{"instagram"},
		Media:     []MediaRef{{Type: "image", URL: "https://cdn.example/post.jpg"}},
	}, Account{
		OrgID:        "org_1",
		ProviderKey:  "meta",
		ConnectionID: "conn_1",
		Status:       AccountStatusConnected,
		Capabilities: []string{"social.post.write"},
		Metadata:     map[string]any{"instagram_user_id": "ig_123"},
	})

	if attempt.Status != AttemptStatusSucceeded {
		t.Fatalf("attempt status = %s, want succeeded: %#v", attempt.Status, attempt)
	}
	if len(executor.calls) != 2 {
		t.Fatalf("action calls = %d, want 2", len(executor.calls))
	}
	if executor.calls[0].Operation != "instagram.media.create" || executor.calls[1].Operation != "instagram.media.publish" {
		t.Fatalf("action sequence = %#v", executor.calls)
	}
	if executor.calls[0].Params["igUserId"] != "ig_123" || executor.calls[0].Body["image_url"] != "https://cdn.example/post.jpg" {
		t.Fatalf("media creation call = %#v", executor.calls[0])
	}
	if executor.calls[1].Body["creation_id"] != "provider-id" {
		t.Fatalf("media publish call = %#v", executor.calls[1])
	}
}

// TestHTTPPublisherSnapchatBlockedWhenLiveDisabled is the honest default: with a
// publish-capable Snapchat account but SNAPCHAT_LIVE_PUBLISHING off, the
// publisher must NOT attempt a real post — it returns a clear blocked attempt.
func TestHTTPPublisherSnapchatBlockedWhenLiveDisabled(t *testing.T) {
	broker := &fakeTokenBroker{token: &TokenLease{AccessToken: "leased-token"}}
	publisher := NewHTTPPublisher(broker, nil, PublisherConfig{}) // SnapchatLivePublishing defaults false

	attempt := publisher.Publish(context.Background(), PublishJob{ID: "job_1", OrgID: "org_1", PostID: "post_1"}, Post{
		ID:        "post_1",
		OrgID:     "org_1",
		Body:      "Publish me",
		Platforms: []string{"snapchat"},
		Media:     []MediaRef{{Type: "image", URL: "https://cdn.example/asset.jpg"}},
	}, Account{
		OrgID:        "org_1",
		ProviderKey:  "snapchat",
		ConnectionID: "conn_1",
		Status:       AccountStatusConnected,
		Capabilities: []string{"social.post.write", "social.media.upload"},
		Metadata:     map[string]any{"public_profile_id": "prof-1"},
	})

	if attempt.Status != AttemptStatusBlocked {
		t.Fatalf("attempt status = %s, want blocked: %#v", attempt.Status, attempt)
	}
	if !strings.Contains(attempt.Message, "SNAPCHAT_LIVE_PUBLISHING") {
		t.Fatalf("message should name the gate, got %q", attempt.Message)
	}
	if !containsString(attempt.Warnings, "snapchat_live_publishing_disabled") {
		t.Fatalf("warnings = %#v, want snapchat_live_publishing_disabled", attempt.Warnings)
	}
}

// TestHTTPPublisherSnapchatMissingProfileID: gate on, but no Public Profile id →
// blocked with a precise message (never a silent success).
func TestHTTPPublisherSnapchatMissingProfileID(t *testing.T) {
	broker := &fakeTokenBroker{token: &TokenLease{AccessToken: "leased-token"}}
	publisher := NewHTTPPublisher(broker, nil, PublisherConfig{SnapchatLivePublishing: true})

	attempt := publisher.Publish(context.Background(), PublishJob{ID: "job_1", OrgID: "org_1", PostID: "post_1"}, Post{
		ID:        "post_1",
		OrgID:     "org_1",
		Body:      "Publish me",
		Platforms: []string{"snapchat"},
		Media:     []MediaRef{{Type: "image", URL: "https://cdn.example/asset.jpg"}},
	}, Account{
		OrgID:        "org_1",
		ProviderKey:  "snapchat",
		ConnectionID: "conn_1",
		Status:       AccountStatusConnected,
		Capabilities: []string{"social.post.write"},
	})

	if attempt.Status != AttemptStatusBlocked {
		t.Fatalf("attempt status = %s, want blocked", attempt.Status)
	}
	if !strings.Contains(attempt.Message, "Public Profile id") {
		t.Fatalf("message should require a Public Profile id, got %q", attempt.Message)
	}
}

// TestHTTPPublisherSnapchatPostsStoryWhenLive exercises the full real pipeline
// against a mock Public Profile API: download media → create encrypted
// container → multipart ADD → FINALIZE → POST /stories.
func TestHTTPPublisherSnapchatPostsStoryWhenLive(t *testing.T) {
	mediaBytes := bytes.Repeat([]byte("verevon-snap-media-"), 3)
	var (
		mu             sync.Mutex
		gotKeyLen      int
		gotIVLen       int
		addActions     []string
		finalizeCalled bool
		uploadedBytes  int
		storyMediaID   string
	)

	var server *httptest.Server
	server = httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.URL.Path == "/asset.jpg":
			_, _ = w.Write(mediaBytes)
		case r.URL.Path == "/v1/public_profiles/prof-1/media" && r.Method == http.MethodPost:
			var body map[string]any
			if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
				t.Fatalf("decode create body: %v", err)
			}
			if key, _ := base64.StdEncoding.DecodeString(stringOrEmpty(body["key"])); true {
				mu.Lock()
				gotKeyLen = len(key)
				mu.Unlock()
			}
			if iv, _ := base64.StdEncoding.DecodeString(stringOrEmpty(body["iv"])); true {
				mu.Lock()
				gotIVLen = len(iv)
				mu.Unlock()
			}
			_ = json.NewEncoder(w).Encode(map[string]any{
				"request_status": "SUCCESS",
				"media_id":       "media-1",
				"add_path":       "/v1/public_profiles/prof-1/media/media-1/multipart-upload",
				"finalize_path":  "/v1/public_profiles/prof-1/media/media-1/multipart-upload",
			})
		case strings.HasSuffix(r.URL.Path, "/multipart-upload") && r.Method == http.MethodPost:
			if err := r.ParseMultipartForm(1 << 20); err != nil {
				t.Fatalf("parse multipart: %v", err)
			}
			action := r.FormValue("action")
			mu.Lock()
			switch action {
			case "ADD":
				addActions = append(addActions, r.FormValue("part_number"))
				if f, _, err := r.FormFile("file"); err == nil {
					data, _ := io.ReadAll(f)
					uploadedBytes += len(data)
				}
			case "FINALIZE":
				finalizeCalled = true
			}
			mu.Unlock()
			_ = json.NewEncoder(w).Encode(map[string]any{"request_status": "SUCCESS"})
		case r.URL.Path == "/v1/public_profiles/prof-1/stories" && r.Method == http.MethodPost:
			var body map[string]any
			_ = json.NewDecoder(r.Body).Decode(&body)
			mu.Lock()
			storyMediaID = stringOrEmpty(body["media_id"])
			mu.Unlock()
			_ = json.NewEncoder(w).Encode(map[string]any{"request_status": "SUCCESS"})
		default:
			t.Fatalf("unexpected request: %s %s", r.Method, r.URL.String())
		}
	}))
	defer server.Close()

	broker := &fakeTokenBroker{token: &TokenLease{AccessToken: "leased-token"}}
	publisher := NewHTTPPublisher(broker, server.Client(), PublisherConfig{
		SnapchatBusinessAPIBaseURL: server.URL + "/v1",
		SnapchatLivePublishing:     true,
	})

	attempt := publisher.Publish(context.Background(), PublishJob{ID: "job_1", OrgID: "org_1", PostID: "post_1"}, Post{
		ID:        "post_1",
		OrgID:     "org_1",
		Title:     "Launch",
		Body:      "Fallback",
		Platforms: []string{"snapchat"},
		Media:     []MediaRef{{Type: "image", URL: server.URL + "/asset.jpg"}},
	}, Account{
		OrgID:        "org_1",
		ProviderKey:  "snapchat",
		ConnectionID: "conn_1",
		Status:       AccountStatusConnected,
		Capabilities: []string{"social.post.write", "social.media.upload"},
		Metadata:     map[string]any{"public_profile_id": "prof-1"},
	})

	if attempt.Status != AttemptStatusSucceeded {
		t.Fatalf("attempt status = %s, want succeeded: %#v", attempt.Status, attempt)
	}
	if attempt.ExternalID != "media-1" {
		t.Fatalf("external id = %q, want media-1", attempt.ExternalID)
	}
	if gotKeyLen != 32 || gotIVLen != 16 {
		t.Fatalf("container key/iv lengths = %d/%d, want 32/16", gotKeyLen, gotIVLen)
	}
	if len(addActions) != 1 {
		t.Fatalf("ADD parts = %#v, want exactly one chunk for a tiny asset", addActions)
	}
	if !finalizeCalled {
		t.Fatalf("FINALIZE was never called")
	}
	// Encrypted+padded bytes must be a whole number of 16-byte AES blocks and
	// strictly larger than the plaintext (PKCS#7 always adds padding).
	if uploadedBytes%16 != 0 || uploadedBytes <= len(mediaBytes) {
		t.Fatalf("uploaded %d encrypted bytes for %d plaintext bytes (want block-aligned, larger)", uploadedBytes, len(mediaBytes))
	}
	if storyMediaID != "media-1" {
		t.Fatalf("story media_id = %q, want media-1", storyMediaID)
	}
}

// TestHTTPPublisherSnapchatSpotlightWhenLive: a video post with
// snapchat_post_type=spotlight routes to the Spotlight endpoint and surfaces
// the pending-review warning.
func TestHTTPPublisherSnapchatSpotlightWhenLive(t *testing.T) {
	var spotlightHit bool
	server := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.URL.Path == "/clip.mp4":
			_, _ = w.Write([]byte("fake-mp4-bytes"))
		case r.URL.Path == "/v1/public_profiles/prof-1/media" && r.Method == http.MethodPost:
			_ = json.NewEncoder(w).Encode(map[string]any{
				"request_status": "SUCCESS",
				"media_id":       "media-2",
				"add_path":       "/v1/public_profiles/prof-1/media/media-2/multipart-upload",
				"finalize_path":  "/v1/public_profiles/prof-1/media/media-2/multipart-upload",
			})
		case strings.HasSuffix(r.URL.Path, "/multipart-upload"):
			_ = json.NewEncoder(w).Encode(map[string]any{"request_status": "SUCCESS"})
		case r.URL.Path == "/v1/public_profiles/prof-1/spotlights" && r.Method == http.MethodPost:
			spotlightHit = true
			var body map[string]any
			_ = json.NewDecoder(r.Body).Decode(&body)
			if body["locale"] != "en_US" {
				t.Fatalf("spotlight locale = %#v, want en_US", body["locale"])
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"request_status": "SUCCESS"})
		default:
			t.Fatalf("unexpected request: %s %s", r.Method, r.URL.String())
		}
	}))
	defer server.Close()

	broker := &fakeTokenBroker{token: &TokenLease{AccessToken: "leased-token"}}
	publisher := NewHTTPPublisher(broker, server.Client(), PublisherConfig{
		SnapchatBusinessAPIBaseURL: server.URL + "/v1",
		SnapchatLivePublishing:     true,
	})

	attempt := publisher.Publish(context.Background(), PublishJob{ID: "job_1", OrgID: "org_1", PostID: "post_1"}, Post{
		ID:        "post_1",
		OrgID:     "org_1",
		Platforms: []string{"snapchat"},
		Previews:  []PlatformPreview{{Platform: "snapchat", Content: "Watch this"}},
		Media:     []MediaRef{{Type: "video", URL: server.URL + "/clip.mp4"}},
	}, Account{
		OrgID:        "org_1",
		ProviderKey:  "snapchat",
		ConnectionID: "conn_1",
		Status:       AccountStatusConnected,
		Capabilities: []string{"social.post.write", "social.media.upload"},
		Metadata:     map[string]any{"public_profile_id": "prof-1", "snapchat_post_type": "spotlight"},
	})

	if attempt.Status != AttemptStatusSucceeded {
		t.Fatalf("attempt status = %s, want succeeded: %#v", attempt.Status, attempt)
	}
	if !spotlightHit {
		t.Fatalf("spotlight endpoint was not called")
	}
	if !containsString(attempt.Warnings, "spotlight_pending_snapchat_review") {
		t.Fatalf("warnings = %#v, want spotlight_pending_snapchat_review", attempt.Warnings)
	}
}

func TestEncryptSnapchatMediaRoundTrip(t *testing.T) {
	plaintext := []byte("the quick brown fox jumps over the lazy snap")
	key := bytes.Repeat([]byte("k"), 32)
	iv := bytes.Repeat([]byte("v"), 16)
	encrypted, err := encryptSnapchatMedia(plaintext, key, iv)
	if err != nil {
		t.Fatalf("encrypt error: %v", err)
	}
	if len(encrypted)%16 != 0 {
		t.Fatalf("ciphertext not block-aligned: %d", len(encrypted))
	}
	// Decrypt (openssl-compatible AES-256-CBC + PKCS#7) must reproduce plaintext.
	block, err := aes.NewCipher(key)
	if err != nil {
		t.Fatalf("cipher error: %v", err)
	}
	decrypted := make([]byte, len(encrypted))
	cipher.NewCBCDecrypter(block, iv).CryptBlocks(decrypted, encrypted)
	pad := int(decrypted[len(decrypted)-1])
	if pad < 1 || pad > 16 || pad > len(decrypted) {
		t.Fatalf("invalid PKCS#7 padding byte %d", pad)
	}
	if got := decrypted[:len(decrypted)-pad]; !bytes.Equal(got, plaintext) {
		t.Fatalf("round-trip = %q, want %q", got, plaintext)
	}
}

func TestChunkBytes(t *testing.T) {
	data := bytes.Repeat([]byte("x"), 70)
	chunks := chunkBytes(data, 32)
	if len(chunks) != 3 {
		t.Fatalf("chunks = %d, want 3", len(chunks))
	}
	if len(chunks[0]) != 32 || len(chunks[1]) != 32 || len(chunks[2]) != 6 {
		t.Fatalf("chunk sizes = %d/%d/%d, want 32/32/6", len(chunks[0]), len(chunks[1]), len(chunks[2]))
	}
	var recombined []byte
	for _, c := range chunks {
		recombined = append(recombined, c...)
	}
	if !bytes.Equal(recombined, data) {
		t.Fatalf("recombined chunks != original")
	}
	if single := chunkBytes(data, 100); len(single) != 1 {
		t.Fatalf("data smaller than chunk size should yield 1 chunk, got %d", len(single))
	}
}

func containsString(items []string, want string) bool {
	for _, item := range items {
		if item == want {
			return true
		}
	}
	return false
}

func stringOrEmpty(v any) string {
	if s, ok := v.(string); ok {
		return s
	}
	return ""
}
