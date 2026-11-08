package spacegrant

import (
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

// WS14-S3: the two Control calls behind a room file. httptest only; no Control.

const testServiceToken = "0123456789abcdef0123456789abcdef"

func TestAttachFilePostsDecisionTokenAndDocumentWithDocumentsAPIPrincipal(t *testing.T) {
	var gotMethod, gotPath, gotServiceID, gotServiceToken, gotContentType string
	var gotBody map[string]string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotMethod, gotPath = r.Method, r.URL.Path
		gotServiceID, gotServiceToken = r.Header.Get("X-Service-Id"), r.Header.Get("X-Service-Token")
		gotContentType = r.Header.Get("Content-Type")
		raw, _ := io.ReadAll(r.Body)
		_ = json.Unmarshal(raw, &gotBody)
		w.WriteHeader(http.StatusOK)
	}))
	defer server.Close()

	client := New(server.URL+"/", testServiceToken)
	if err := client.AttachFile(context.Background(), " decision-token ", " doc-1 "); err != nil {
		t.Fatalf("AttachFile: %v", err)
	}
	if gotMethod != http.MethodPost || gotPath != "/api/v1/internal/spaces/file-grant" {
		t.Fatalf("request = %s %s", gotMethod, gotPath)
	}
	if gotServiceID != "documents-api" || gotServiceToken != testServiceToken {
		t.Fatalf("service identity = %q / token matches %v", gotServiceID, gotServiceToken == testServiceToken)
	}
	if gotContentType != "application/json" {
		t.Fatalf("content type = %q", gotContentType)
	}
	if len(gotBody) != 2 || gotBody["decision_token"] != "decision-token" || gotBody["document_id"] != "doc-1" {
		t.Fatalf("body = %v, want exactly decision_token and document_id (trimmed)", gotBody)
	}
}

func TestAttachFileMapsControlAnswers(t *testing.T) {
	cases := []struct {
		name        string
		status      int
		body        string
		wantNil     bool
		wantDenied  bool
		wantCode    string
		wantUnavail bool
	}{
		{name: "200 attaches", status: http.StatusOK, wantNil: true},
		{name: "403 carries Control's code", status: http.StatusForbidden, body: `{"code":"space_member_required"}`, wantDenied: true, wantCode: "space_member_required"},
		{name: "403 without a code is still a denial", status: http.StatusForbidden, body: `not json`, wantDenied: true},
		{name: "404 means Control's flag is off", status: http.StatusNotFound, wantUnavail: true},
		{name: "500", status: http.StatusInternalServerError, wantUnavail: true},
		{name: "503", status: http.StatusServiceUnavailable, wantUnavail: true},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				w.WriteHeader(tc.status)
				_, _ = w.Write([]byte(tc.body))
			}))
			defer server.Close()
			err := New(server.URL, testServiceToken).AttachFile(context.Background(), "token", "doc-1")
			if tc.wantNil {
				if err != nil {
					t.Fatalf("err = %v", err)
				}
				return
			}
			var denied *DeniedError
			isDenied := errors.As(err, &denied)
			if isDenied != tc.wantDenied {
				t.Fatalf("denied = %v (err %v), want %v", isDenied, err, tc.wantDenied)
			}
			if tc.wantDenied && denied.Code != tc.wantCode {
				t.Fatalf("denial code = %q, want %q", denied.Code, tc.wantCode)
			}
			if tc.wantUnavail && !errors.Is(err, ErrUnavailable) {
				t.Fatalf("err = %v, want ErrUnavailable", err)
			}
			if tc.wantUnavail && errors.As(err, &denied) {
				t.Fatal("an unavailable answer must never read as a denial")
			}
		})
	}
}

func TestAttachFileFailsClosedWithoutConfigOrInputsAndOnTransportError(t *testing.T) {
	if err := New("", testServiceToken).AttachFile(context.Background(), "t", "d"); !errors.Is(err, ErrNotConfigured) {
		t.Fatalf("no url: %v", err)
	}
	if err := New("http://127.0.0.1:1", "").AttachFile(context.Background(), "t", "d"); !errors.Is(err, ErrNotConfigured) {
		t.Fatalf("no service token: %v", err)
	}
	var nilClient *Client
	if err := nilClient.AttachFile(context.Background(), "t", "d"); !errors.Is(err, ErrNotConfigured) {
		t.Fatalf("nil client: %v", err)
	}
	client := New("http://127.0.0.1:1", testServiceToken)
	if err := client.AttachFile(context.Background(), " ", "d"); err == nil {
		t.Fatal("empty decision token accepted")
	}
	if err := client.AttachFile(context.Background(), "t", " "); err == nil {
		t.Fatal("empty document id accepted")
	}
	// An unreachable Control is "unavailable", never a silent success.
	if err := client.AttachFile(context.Background(), "t", "d"); !errors.Is(err, ErrUnavailable) {
		t.Fatalf("transport failure = %v, want ErrUnavailable", err)
	}
}

func TestRevokeFileSendsOwnerAssertedDeleteWithBearerAndSignedDelegation(t *testing.T) {
	now := time.Date(2026, 11, 2, 6, 0, 0, 0, time.UTC)
	called := false
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		called = true
		if r.Method != http.MethodDelete || r.URL.Path != "/api/v1/internal/authz/grant" {
			t.Errorf("request = %s %s", r.Method, r.URL.Path)
		}
		q := r.URL.Query()
		want := map[string]string{
			"org_id": "org-1", "resource_type": "document", "resource_id": "doc-1",
			"subject_id": "space:org-1:room-7", "subject_type": "space",
		}
		for k, v := range want {
			if q.Get(k) != v {
				t.Errorf("query %s = %q, want %q", k, q.Get(k), v)
			}
		}
		if got := r.Header.Get("Authorization"); got != "Bearer uploader-token" {
			t.Errorf("Authorization = %q, want the uploader's bearer", got)
		}
		if r.Header.Get("X-Service-Id") != "documents-api" || r.Header.Get("X-Service-Token") != testServiceToken {
			t.Error("service identity missing")
		}
		if r.Header.Get("X-User-Id") != "user-1" || r.Header.Get("X-Org-Id") != "org-1" {
			t.Error("delegated user or tenant not bound")
		}
		if r.Header.Get("X-Delegation-Version") != "v2" ||
			r.Header.Get("X-Delegation-Operation") != "authz:revoke" ||
			r.Header.Get("X-Delegation-Resource-Type") != "document" ||
			r.Header.Get("X-Delegation-Resource-Id") != "doc-1" ||
			r.Header.Get("X-Delegation-ZDR") != "true" {
			t.Error("delegation metadata does not name authz:revoke on the document")
		}
		nonce := r.Header.Get("X-Delegation-Nonce")
		if len(nonce) < 16 {
			t.Error("delegation nonce missing")
		}
		emptyDigest := base64.RawURLEncoding.EncodeToString(sha256.New().Sum(nil))
		if r.Header.Get("X-Delegation-Body-SHA256") != emptyDigest {
			t.Errorf("body digest = %q, want the empty-body digest", r.Header.Get("X-Delegation-Body-SHA256"))
		}
		// Re-derive the signature the way Control's service_delegation.go does.
		canonical := strings.Join([]string{
			"v2", "documents-api", "user-core", now.Format(time.RFC3339),
			http.MethodDelete, r.URL.RequestURI(), "user-1", "org-1", "authz:revoke",
			"document", "doc-1", r.Header.Get("X-Delegation-Reason"), "true", nonce, emptyDigest,
		}, "\n")
		mac := hmac.New(sha256.New, []byte(testServiceToken))
		_, _ = mac.Write([]byte(canonical))
		if want := base64.RawURLEncoding.EncodeToString(mac.Sum(nil)); r.Header.Get("X-Delegation-Signature") != want {
			t.Error("delegation HMAC does not verify against the canonical form")
		}
		if r.Header.Get("X-Delegation-Timestamp") != now.Format(time.RFC3339) {
			t.Errorf("timestamp = %q", r.Header.Get("X-Delegation-Timestamp"))
		}
		w.WriteHeader(http.StatusOK)
	}))
	defer server.Close()

	client := New(server.URL, testServiceToken)
	client.now = func() time.Time { return now }
	if err := client.RevokeFile(context.Background(), "org-1", "user-1", "Bearer uploader-token", "doc-1", "space:org-1:room-7"); err != nil {
		t.Fatalf("RevokeFile: %v", err)
	}
	if !called {
		t.Fatal("Control was never called")
	}
}

func TestRevokeFileMapsControlAnswersAndRefusesUnsafeInputs(t *testing.T) {
	status := http.StatusOK
	hits := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		hits++
		w.WriteHeader(status)
	}))
	defer server.Close()
	client := New(server.URL, testServiceToken)
	revoke := func(authorization string) error {
		return client.RevokeFile(context.Background(), "org-1", "user-1", authorization, "doc-1", "space:org-1:room-7")
	}

	status = http.StatusForbidden
	var denied *DeniedError
	if err := revoke("Bearer t"); !errors.As(err, &denied) {
		t.Fatalf("403 = %v, want DeniedError", err)
	}
	for _, s := range []int{http.StatusNotFound, http.StatusInternalServerError} {
		status = s
		if err := revoke("Bearer t"); !errors.Is(err, ErrUnavailable) {
			t.Fatalf("%d = %v, want ErrUnavailable", s, err)
		}
	}

	hits = 0
	for _, bad := range []string{"", "Bearer ", "Basic abc", "Bearer t\r\nX-Forged: 1"} {
		if err := revoke(bad); err == nil {
			t.Fatalf("authorization %q was accepted", bad)
		}
	}
	if err := client.RevokeFile(context.Background(), "org-1", "", "Bearer t", "doc-1", "space:x"); err == nil {
		t.Fatal("empty user accepted")
	}
	if err := client.RevokeFile(context.Background(), "org-1", "user-1", "Bearer t", "doc-1", ""); err == nil {
		t.Fatal("empty Space accepted")
	}
	if hits != 0 {
		t.Fatalf("Control was called %d times for refused inputs", hits)
	}
	if err := New("", testServiceToken).RevokeFile(context.Background(), "o", "u", "Bearer t", "d", "s"); !errors.Is(err, ErrNotConfigured) {
		t.Fatalf("not configured = %v", err)
	}
}
