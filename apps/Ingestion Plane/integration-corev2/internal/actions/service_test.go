package actions

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"reflect"
	"strings"
	"testing"

	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/store"
)

func TestExecuteMicrosoftMailSendTranslatesVerevonEnvelope(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v1.0/me/sendMail" {
			t.Fatalf("path = %s, want /v1.0/me/sendMail", r.URL.Path)
		}
		var body map[string]any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("Decode body error: %v", err)
		}
		message, ok := body["message"].(map[string]any)
		if !ok {
			t.Fatalf("body = %#v, want Graph message envelope", body)
		}
		if message["subject"] != "Delivery update" {
			t.Fatalf("message subject = %#v", message["subject"])
		}
		headers, _ := message["internetMessageHeaders"].([]any)
		if len(headers) != 1 {
			t.Fatalf("internetMessageHeaders = %#v, want one opaque correlation header", message["internetMessageHeaders"])
		}
		bodyContent, _ := message["body"].(map[string]any)
		if bodyContent["contentType"] != "HTML" || bodyContent["content"] != "<p>Checked</p>" {
			t.Fatalf("message body = %#v, want HTML content", bodyContent)
		}
		recipients, _ := message["toRecipients"].([]any)
		if len(recipients) != 1 {
			t.Fatalf("toRecipients = %#v, want one recipient", message["toRecipients"])
		}
		w.WriteHeader(http.StatusAccepted)
	}))
	defer server.Close()

	service := NewService(config.Config{MicrosoftGraphBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection: store.Connection{ProviderKey: "microsoft"}, AccessToken: "token", Operation: "mail.send",
		Body: map[string]any{"subject": "Delivery update", "bodyText": "Checked", "bodyHtml": "<p>Checked</p>", "to": []string{"customer@example.com"}, "correlationId": "outintent_123"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteGoogleGmailSendTranslatesVerevonEnvelope(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/gmail/v1/users/me/messages/send" {
			t.Fatalf("path = %s, want Gmail send endpoint", r.URL.Path)
		}
		var body map[string]any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("Decode body error: %v", err)
		}
		raw, ok := body["raw"].(string)
		if !ok || raw == "" {
			t.Fatalf("body = %#v, want RFC-822 raw message", body)
		}
		decoded, err := base64.RawURLEncoding.DecodeString(raw)
		if err != nil {
			t.Fatalf("decode raw message: %v", err)
		}
		wire := string(decoded)
		for _, required := range []string{"To: customer@example.com", "Subject: Delivery update", "In-Reply-To: <customer-message@example.com>", "References: <root@example.com> <customer-message@example.com>", "X-Verevon-Outbound-Intent: outintent_123", "Checked"} {
			if !strings.Contains(wire, required) {
				t.Fatalf("raw message missing %q:\n%s", required, wire)
			}
		}
		if body["threadId"] != "gmail-thread-1" {
			t.Fatalf("threadId = %#v, want gmail-thread-1", body["threadId"])
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"id": "gmail-message-1"})
	}))
	defer server.Close()

	service := NewService(config.Config{GoogleAPIBaseURL: server.URL}, server.Client())
	result, err := service.Execute(context.Background(), ExecuteInput{
		Connection: store.Connection{ProviderKey: "google"}, AccessToken: "token", Operation: "gmail.send",
		Body: map[string]any{"subject": "Delivery update", "bodyText": "Checked", "to": []string{"customer@example.com"}, "threadId": "gmail-thread-1", "inReplyTo": "<customer-message@example.com>", "references": "<root@example.com> <customer-message@example.com>", "correlationId": "outintent_123"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
	payload, _ := result.Result.(map[string]any)
	if payload["id"] != "gmail-message-1" {
		t.Fatalf("result = %#v, want Gmail message response", result.Result)
	}
}

func TestMailSendPayloadRejectsHeaderInjection(t *testing.T) {
	_, err := gmailSendPayload(map[string]any{
		"subject":  "Delivery update\r\nBcc: attacker@example.com",
		"bodyText": "Checked",
		"to":       []string{"customer@example.com"},
	})
	if err == nil || !strings.Contains(err.Error(), "subject cannot contain line breaks") {
		t.Fatalf("gmailSendPayload error = %v, want subject header validation", err)
	}

	_, err = microsoftMailSendPayload(map[string]any{
		"subject":  "Delivery update",
		"bodyText": "Checked",
		"to":       []string{"customer@example.com\r\nBcc: attacker@example.com"},
	})
	if err == nil || !strings.Contains(err.Error(), "invalid mail recipient") {
		t.Fatalf("microsoftMailSendPayload error = %v, want recipient validation", err)
	}

	_, err = gmailSendPayload(map[string]any{
		"subject":    "Delivery update",
		"bodyText":   "Checked",
		"to":         []string{"customer@example.com"},
		"references": "<root@example.com>\r\nBcc: attacker@example.com",
	})
	if err == nil || !strings.Contains(err.Error(), "references cannot contain line breaks") {
		t.Fatalf("gmailSendPayload error = %v, want reference header validation", err)
	}

	_, err = gmailSendPayload(map[string]any{
		"subject":       "Delivery update",
		"bodyText":      "Checked",
		"to":            []string{"customer@example.com"},
		"correlationId": "outintent_123\r\nX-Injected: true",
	})
	if err == nil || !strings.Contains(err.Error(), "correlationId contains invalid characters") {
		t.Fatalf("gmailSendPayload error = %v, want correlation token validation", err)
	}
}

func TestMailSendPayloadKeepsNativeProviderBodiesImmutable(t *testing.T) {
	graphNative := map[string]any{"message": map[string]any{"subject": "native"}, "saveToSentItems": false}
	graphPayload, err := microsoftMailSendPayload(graphNative)
	if err != nil {
		t.Fatalf("microsoftMailSendPayload error: %v", err)
	}
	graphPayload["saveToSentItems"] = true
	if graphNative["saveToSentItems"] != false {
		t.Fatalf("native Graph payload was mutated: %#v", graphNative)
	}

	gmailNative := map[string]any{"raw": "already-encoded", "threadId": "thread-1"}
	gmailPayload, err := gmailSendPayload(gmailNative)
	if err != nil {
		t.Fatalf("gmailSendPayload error: %v", err)
	}
	if !reflect.DeepEqual(gmailPayload, gmailNative) {
		t.Fatalf("Gmail payload = %#v, want %#v", gmailPayload, gmailNative)
	}
	gmailPayload["threadId"] = "thread-2"
	if gmailNative["threadId"] != "thread-1" {
		t.Fatalf("native Gmail payload was mutated: %#v", gmailNative)
	}
}

func TestExecuteSlackChannelsList(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/conversations.list" {
			t.Fatalf("path = %s, want /conversations.list", r.URL.Path)
		}
		if got := r.URL.Query().Get("types"); got != "public_channel" {
			t.Fatalf("types = %q, want public_channel", got)
		}
		if got := r.Header.Get("Authorization"); got != "Bearer token" {
			t.Fatalf("Authorization = %q, want Bearer token", got)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"ok": true})
	}))
	defer server.Close()

	service := NewService(config.Config{SlackAPIBaseURL: server.URL}, server.Client())
	result, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "slack"},
		AccessToken: "token",
		Operation:   "channels.list",
		Params:      map[string]any{"types": "public_channel"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
	if result.ProviderKey != "slack" || result.Operation != "channels.list" {
		t.Fatalf("result = %#v", result)
	}
}

func TestExecuteLinkedInPostCreateAddsRestliHeaders(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/rest/posts" {
			t.Fatalf("path = %s, want /rest/posts", r.URL.Path)
		}
		if got := r.Header.Get("Authorization"); got != "Bearer token" {
			t.Fatalf("Authorization = %q, want Bearer token", got)
		}
		if got := r.Header.Get("X-Restli-Protocol-Version"); got != "2.0.0" {
			t.Fatalf("X-Restli-Protocol-Version = %q, want 2.0.0", got)
		}
		if got := r.Header.Get("LinkedIn-Version"); got != "202606" {
			t.Fatalf("LinkedIn-Version = %q, want 202606", got)
		}
		var body map[string]any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("Decode body error: %v", err)
		}
		if body["author"] != "urn:li:person:abc" {
			t.Fatalf("body = %#v, want author", body)
		}
		w.Header().Set("x-restli-id", "urn:li:share:1")
		w.WriteHeader(http.StatusCreated)
	}))
	defer server.Close()

	service := NewService(config.Config{LinkedInAPIBaseURL: server.URL, LinkedInMarketingVersion: "202606"}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "linkedin"},
		AccessToken: "token",
		Operation:   "posts.create",
		Body:        map[string]any{"author": "urn:li:person:abc", "commentary": "hello"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteLinkedInVerificationReport(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/rest/verificationReport" {
			t.Fatalf("path = %s, want /rest/verificationReport", r.URL.Path)
		}
		if got := r.URL.Query()["verificationCriteria"]; len(got) != 2 || got[0] != "IDENTITY" || got[1] != "WORKPLACE" {
			t.Fatalf("verificationCriteria = %#v, want IDENTITY and WORKPLACE", got)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"verifications": []string{"IDENTITY"}})
	}))
	defer server.Close()

	service := NewService(config.Config{LinkedInAPIBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "linkedin"},
		AccessToken: "token",
		Operation:   "verification.report",
		Params:      map[string]any{"verificationCriteria": []string{"IDENTITY", "WORKPLACE"}},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteLinkedInAdAccountsList(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/rest/adAccounts" {
			t.Fatalf("path = %s, want /rest/adAccounts", r.URL.Path)
		}
		if got := r.URL.Query().Get("pageSize"); got != "25" {
			t.Fatalf("pageSize = %q, want 25", got)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"elements": []map[string]any{{"id": 123}}})
	}))
	defer server.Close()

	service := NewService(config.Config{LinkedInAPIBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "linkedin"},
		AccessToken: "token",
		Operation:   "ads.accounts",
		Params:      map[string]any{"pageSize": 25},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteLinkedInAdCampaignCreate(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/rest/adAccounts/123/adCampaigns" {
			t.Fatalf("path = %s, want /rest/adAccounts/123/adCampaigns", r.URL.Path)
		}
		if got := r.Header.Get("X-Restli-Protocol-Version"); got != "2.0.0" {
			t.Fatalf("X-Restli-Protocol-Version = %q, want 2.0.0", got)
		}
		var body map[string]any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("Decode body error: %v", err)
		}
		if body["name"] != "Awareness" {
			t.Fatalf("body = %#v, want campaign name", body)
		}
		w.Header().Set("x-restli-id", "456")
		w.WriteHeader(http.StatusCreated)
	}))
	defer server.Close()

	service := NewService(config.Config{LinkedInAPIBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "linkedin"},
		AccessToken: "token",
		Operation:   "ads.campaign.create",
		Params:      map[string]any{"accountId": "123"},
		Body:        map[string]any{"name": "Awareness"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteSnapchatOrganizationsUsesAdsBase(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/me/organizations" {
			t.Fatalf("path = %s, want /me/organizations", r.URL.Path)
		}
		if got := r.Header.Get("Authorization"); got != "Bearer token" {
			t.Fatalf("Authorization = %q, want Bearer token", got)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"organizations": []map[string]any{{"organization": map[string]any{"id": "org1"}}}})
	}))
	defer server.Close()

	service := NewService(config.Config{SnapchatAPIBaseURL: server.URL}, server.Client())
	result, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "snapchat"},
		AccessToken: "token",
		Operation:   "organizations",
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
	if result.ProviderKey != "snapchat" || result.Operation != "organizations" {
		t.Fatalf("result = %#v", result)
	}
}

func TestExecuteSnapchatAdsCreativeCreatePostsJSON(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/adaccounts/acc-1/creatives" {
			t.Fatalf("path = %s, want /adaccounts/acc-1/creatives", r.URL.Path)
		}
		if r.Method != http.MethodPost {
			t.Fatalf("method = %s, want POST", r.Method)
		}
		var body map[string]any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("Decode body error: %v", err)
		}
		if body["top_snap_media_id"] != "media-1" || body["type"] != "SNAP_AD" {
			t.Fatalf("body = %#v, want creative fields", body)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"request_status": "success"})
	}))
	defer server.Close()

	service := NewService(config.Config{SnapchatAPIBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "snapchat"},
		AccessToken: "token",
		Operation:   "snapchat.ads.creative.create",
		Params:      map[string]any{"adAccountId": "acc-1"},
		Body:        map[string]any{"name": "Health", "type": "SNAP_AD", "top_snap_media_id": "media-1"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteSnapchatSpotlightPostUsesBusinessBase(t *testing.T) {
	adsHit := false
	adsServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		adsHit = true
		w.WriteHeader(http.StatusOK)
	}))
	defer adsServer.Close()
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/public_profiles/prof-1/spotlights" {
			t.Fatalf("path = %s, want /public_profiles/prof-1/spotlights", r.URL.Path)
		}
		var body map[string]any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("Decode body error: %v", err)
		}
		if body["media_id"] != "m-1" || body["locale"] != "en_US" {
			t.Fatalf("body = %#v, want spotlight fields", body)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"request_status": "SUCCESS"})
	}))
	defer server.Close()

	service := NewService(config.Config{SnapchatAPIBaseURL: adsServer.URL, SnapchatBusinessAPIBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "snapchat"},
		AccessToken: "token",
		Operation:   "spotlight.post",
		Params:      map[string]any{"profileId": "prof-1"},
		Body:        map[string]any{"media_id": "m-1", "description": "hi", "locale": "en_US"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
	if adsHit {
		t.Fatalf("Public Profile posting must not hit the Ads API host")
	}
}

func TestExecuteSnapchatMissingProfileIDErrors(t *testing.T) {
	service := NewService(config.Config{SnapchatBusinessAPIBaseURL: "https://businessapi.example"}, http.DefaultClient)
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "snapchat"},
		AccessToken: "token",
		Operation:   "story.post",
		Body:        map[string]any{"media_id": "m-1"},
	})
	if err == nil {
		t.Fatalf("expected error when profileId is missing")
	}
}

func TestExecuteGitHubRepoEscapesPath(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/repos/triodelab/verevon" {
			t.Fatalf("path = %s, want /repos/triodelab/verevon", r.URL.Path)
		}
		if got := r.Header.Get("X-GitHub-Api-Version"); got == "" {
			t.Fatalf("missing GitHub API version header")
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"full_name": "triodelab/verevon"})
	}))
	defer server.Close()

	service := NewService(config.Config{GitHubAPIBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "github"},
		AccessToken: "token",
		Operation:   "repo",
		Params:      map[string]any{"owner": "triodelab", "repo": "verevon"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteSlackUserInfo(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/users.info" {
			t.Fatalf("path = %s, want /users.info", r.URL.Path)
		}
		if got := r.URL.Query().Get("user"); got != "U123" {
			t.Fatalf("user = %q, want U123", got)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"ok": true})
	}))
	defer server.Close()

	service := NewService(config.Config{SlackAPIBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "slack"},
		AccessToken: "token",
		Operation:   "user",
		Params:      map[string]any{"userId": "U123"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteGitHubTeams(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/orgs/triodelab/teams" {
			t.Fatalf("path = %s, want /orgs/triodelab/teams", r.URL.Path)
		}
		_ = json.NewEncoder(w).Encode([]map[string]any{})
	}))
	defer server.Close()

	service := NewService(config.Config{GitHubAPIBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "github"},
		AccessToken: "token",
		Operation:   "teams",
		Params:      map[string]any{"org": "triodelab"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteGitHubContentsGetEscapesPathSegments(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/repos/triodelab/verevon/contents/docs/README.md" {
			t.Fatalf("path = %s, want /repos/triodelab/verevon/contents/docs/README.md", r.URL.Path)
		}
		if got := r.URL.Query().Get("ref"); got != "main" {
			t.Fatalf("ref = %q, want main", got)
		}
		if got := r.Header.Get("X-GitHub-Api-Version"); got == "" {
			t.Fatalf("missing GitHub API version header")
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"path": "docs/README.md"})
	}))
	defer server.Close()

	service := NewService(config.Config{GitHubAPIBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "github"},
		AccessToken: "token",
		Operation:   "contents.get",
		Params:      map[string]any{"owner": "triodelab", "repo": "verevon", "path": "docs/README.md", "ref": "main"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteGitHubIssueCreatePostsJSON(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/repos/triodelab/verevon/issues" {
			t.Fatalf("path = %s, want /repos/triodelab/verevon/issues", r.URL.Path)
		}
		if got := r.Header.Get("Authorization"); got != "Bearer token" {
			t.Fatalf("Authorization = %q, want Bearer token", got)
		}
		var body map[string]any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("Decode body error: %v", err)
		}
		if body["title"] != "Fix checkout" {
			t.Fatalf("body = %#v, want issue title", body)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"number": 42})
	}))
	defer server.Close()

	service := NewService(config.Config{GitHubAPIBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "github"},
		AccessToken: "token",
		Operation:   "issues.create",
		Params:      map[string]any{"owner": "triodelab", "repo": "verevon"},
		Body:        map[string]any{"title": "Fix checkout"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteGitHubPullsList(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/repos/triodelab/verevon/pulls" {
			t.Fatalf("path = %s, want /repos/triodelab/verevon/pulls", r.URL.Path)
		}
		if got := r.URL.Query().Get("state"); got != "open" {
			t.Fatalf("state = %q, want open", got)
		}
		_ = json.NewEncoder(w).Encode([]map[string]any{{"number": 7}})
	}))
	defer server.Close()

	service := NewService(config.Config{GitHubAPIBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "github"},
		AccessToken: "token",
		Operation:   "pulls.list",
		Params:      map[string]any{"owner": "triodelab", "repo": "verevon", "state": "open"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteOktaUsersUsesAPIToken(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v1/users" {
			t.Fatalf("path = %s, want /api/v1/users", r.URL.Path)
		}
		if got := r.Header.Get("Authorization"); got != "SSWS okta-token" {
			t.Fatalf("Authorization = %q, want SSWS okta-token", got)
		}
		_ = json.NewEncoder(w).Encode([]map[string]any{{"id": "00u1"}})
	}))
	defer server.Close()

	service := NewService(config.Config{OktaAPIBaseURL: server.URL, OktaAPIToken: "okta-token"}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection: store.Connection{
			ProviderKey:  "okta",
			Capabilities: []string{"directory.read"},
		},
		Operation: "users",
		Params:    map[string]any{"limit": 10},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteMetaPagesPostUsesPageAccessToken(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/page-1":
			if got := r.Header.Get("Authorization"); got != "Bearer user-token" {
				t.Fatalf("page token lookup Authorization = %q, want Bearer user-token", got)
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"access_token": "page-token"})
		case "/page-1/feed":
			if got := r.Header.Get("Authorization"); got != "Bearer page-token" {
				t.Fatalf("post Authorization = %q, want Bearer page-token", got)
			}
			if err := r.ParseForm(); err != nil {
				t.Fatalf("ParseForm error: %v", err)
			}
			if got := r.Form.Get("message"); got != "hello page" {
				t.Fatalf("message = %q, want hello page", got)
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"id": "page-1_post-1"})
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	service := NewService(config.Config{FacebookAPIBaseURL: server.URL}, server.Client())
	result, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "meta"},
		AccessToken: "user-token",
		Operation:   "pages.post",
		Params:      map[string]any{"pageId": "page-1"},
		Body:        map[string]any{"message": "hello page"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
	if result.ProviderKey != "meta" || result.Operation != "pages.post" {
		t.Fatalf("result = %#v", result)
	}
}

func TestExecuteMetaWhatsAppSendUsesCloudAPIJSON(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/phone-1/messages" {
			t.Fatalf("path = %s, want /phone-1/messages", r.URL.Path)
		}
		if got := r.Header.Get("Authorization"); got != "Bearer token" {
			t.Fatalf("Authorization = %q, want Bearer token", got)
		}
		var body map[string]any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("Decode body error: %v", err)
		}
		if body["messaging_product"] != "whatsapp" {
			t.Fatalf("body = %#v, want WhatsApp payload", body)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"messages": []map[string]any{{"id": "wamid.1"}}})
	}))
	defer server.Close()

	service := NewService(config.Config{FacebookAPIBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "meta"},
		AccessToken: "token",
		Operation:   "whatsapp.messages.send",
		Params:      map[string]any{"phoneNumberId": "phone-1"},
		Body:        map[string]any{"messaging_product": "whatsapp", "to": "+15551234567", "type": "text", "text": map[string]any{"body": "hi"}},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteMetaWhatsAppSendDefaultsMessagingProduct(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body map[string]any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("Decode body error: %v", err)
		}
		// Meta requires messaging_product on every send and does not
		// default it -- a caller that omits it would otherwise get a
		// silent Graph API 400 with no clear cause.
		if body["messaging_product"] != "whatsapp" {
			t.Fatalf("body = %#v, want messaging_product defaulted to whatsapp", body)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"messages": []map[string]any{{"id": "wamid.1"}}})
	}))
	defer server.Close()

	service := NewService(config.Config{FacebookAPIBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "meta"},
		AccessToken: "token",
		Operation:   "whatsapp.messages.send",
		Params:      map[string]any{"phoneNumberId": "phone-1"},
		Body:        map[string]any{"to": "+15551234567", "type": "text", "text": map[string]any{"body": "hi"}},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteMetaWhatsAppSendRequiresToAndType(t *testing.T) {
	service := NewService(config.Config{FacebookAPIBaseURL: "https://graph.facebook.test"}, http.DefaultClient)
	for _, body := range []map[string]any{
		{"type": "text", "text": map[string]any{"body": "hi"}},
		{"to": "+15551234567"},
	} {
		_, err := service.Execute(context.Background(), ExecuteInput{
			Connection:  store.Connection{ProviderKey: "meta"},
			AccessToken: "token",
			Operation:   "whatsapp.messages.send",
			Params:      map[string]any{"phoneNumberId": "phone-1"},
			Body:        body,
		})
		if err == nil {
			t.Fatalf("expected an error for incomplete body %#v", body)
		}
	}
}

func TestExecuteMetaAdsCampaignsNormalizesAdAccountPath(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/act_123/campaigns" {
			t.Fatalf("path = %s, want /act_123/campaigns", r.URL.Path)
		}
		if got := r.URL.Query().Get("fields"); got == "" {
			t.Fatalf("missing fields query")
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"data": []map[string]any{{"id": "cmp-1"}}})
	}))
	defer server.Close()

	service := NewService(config.Config{FacebookAPIBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "meta"},
		AccessToken: "token",
		Operation:   "ads.campaigns",
		Params:      map[string]any{"adAccountId": "act_123"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteMetaThreadsPublishUsesThreadsBaseURL(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/me/threads_publish" {
			t.Fatalf("path = %s, want /me/threads_publish", r.URL.Path)
		}
		if err := r.ParseForm(); err != nil {
			t.Fatalf("ParseForm error: %v", err)
		}
		if got := r.Form.Get("creation_id"); got != "container-1" {
			t.Fatalf("creation_id = %q, want container-1", got)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"id": "thread-1"})
	}))
	defer server.Close()

	service := NewService(config.Config{MetaThreadsAPIBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "meta"},
		AccessToken: "token",
		Operation:   "threads.publish",
		Body:        map[string]any{"creation_id": "container-1"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
}

func TestExecuteMetaThreadsContainerStatusUsesThreadsBaseURL(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/container-1" {
			t.Fatalf("path = %s, want /container-1", r.URL.Path)
		}
		if got := r.URL.Query().Get("fields"); got != "id,status,error_message" {
			t.Fatalf("fields = %q, want id,status,error_message", got)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"id": "container-1", "status": "FINISHED"})
	}))
	defer server.Close()

	service := NewService(config.Config{MetaThreadsAPIBaseURL: server.URL}, server.Client())
	result, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "meta"},
		AccessToken: "token",
		Operation:   "threads.container.status",
		Params:      map[string]any{"creationId": "container-1"},
	})
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
	body, ok := result.Result.(map[string]any)
	if !ok || body["status"] != "FINISHED" {
		t.Fatalf("result = %#v, want status FINISHED", result.Result)
	}
}

func TestExecuteMetaThreadsContainerStatusRequiresCreationID(t *testing.T) {
	service := NewService(config.Config{MetaThreadsAPIBaseURL: "https://graph.threads.test"}, http.DefaultClient)
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "meta"},
		AccessToken: "token",
		Operation:   "threads.container.status",
	})
	if err == nil {
		t.Fatal("expected an error when creationId is missing")
	}
}

func TestExecuteRejectsUnsupportedOperation(t *testing.T) {
	service := NewService(config.Config{}, http.DefaultClient)
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "slack"},
		AccessToken: "token",
		Operation:   "admin.openProxy",
	})
	if err == nil {
		t.Fatalf("expected unsupported operation error")
	}
}

func TestExecuteInstagramMessagesSendResolvesLinkedPage(t *testing.T) {
	var sentBody map[string]any
	var sentAuth string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/me/accounts":
			if got := r.Header.Get("Authorization"); got != "Bearer user-token" {
				t.Fatalf("accounts Authorization = %q, want user token", got)
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"data": []any{
				map[string]any{"id": "page-77", "access_token": "page-token-77",
					"instagram_business_account": map[string]any{"id": "1784140000"}},
				map[string]any{"id": "page-88", "access_token": "page-token-88"},
			}})
		case "/page-77/messages":
			sentAuth = r.Header.Get("Authorization")
			_ = json.NewDecoder(r.Body).Decode(&sentBody)
			_ = json.NewEncoder(w).Encode(map[string]any{"message_id": "ig_m_1"})
		default:
			t.Fatalf("unexpected path %s", r.URL.Path)
		}
	}))
	defer server.Close()

	service := NewService(config.Config{FacebookAPIBaseURL: server.URL}, server.Client())
	result, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "instagram"},
		AccessToken: "user-token",
		Operation:   "instagram.messages.send",
		Params:      map[string]any{"igAccountId": "1784140000"},
		Body: map[string]any{
			"recipient":      map[string]any{"id": "8930000001"},
			"message":        map[string]any{"text": "Ja, på lager!"},
			"messaging_type": "RESPONSE",
		},
	})
	if err != nil {
		t.Fatalf("execute: %v", err)
	}
	_ = result
	if sentAuth != "Bearer page-token-77" {
		t.Fatalf("send Authorization = %q, want the LINKED PAGE token", sentAuth)
	}
	recip, _ := sentBody["recipient"].(map[string]any)
	if recip["id"] != "8930000001" {
		t.Fatalf("recipient = %#v, want the IGSID", sentBody["recipient"])
	}
}

func TestExecuteInstagramMessagesSendNoLinkedPageFailsClosed(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/me/accounts" {
			t.Fatalf("unexpected path %s", r.URL.Path)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"data": []any{
			map[string]any{"id": "page-88", "access_token": "page-token-88"},
		}})
	}))
	defer server.Close()

	service := NewService(config.Config{FacebookAPIBaseURL: server.URL}, server.Client())
	_, err := service.Execute(context.Background(), ExecuteInput{
		Connection:  store.Connection{ProviderKey: "instagram"},
		AccessToken: "user-token",
		Operation:   "instagram.messages.send",
		Params:      map[string]any{"igAccountId": "1784140000"},
		Body:        map[string]any{"recipient": map[string]any{"id": "x"}},
	})
	if err == nil {
		t.Fatal("want error when no linked page matches the IG account")
	}
}
