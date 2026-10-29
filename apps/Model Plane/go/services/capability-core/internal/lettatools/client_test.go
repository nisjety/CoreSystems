package lettatools

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

func TestConfigFromLookupIsDisabledByDefault(t *testing.T) {
	t.Parallel()

	config, err := ConfigFromLookup(func(string) string { return "" })
	if err != nil {
		t.Fatal(err)
	}
	if config.Enabled {
		t.Fatal("Letta tool search must be opt-in")
	}
}

func TestConfigFromLookupAcceptsBoundedHTTPSDefaults(t *testing.T) {
	t.Parallel()

	values := map[string]string{
		"LETTA_TOOL_SEARCH_ENABLED": "true",
		"LETTA_TOOL_SEARCH_URL":     "https://api.letta.example/v1/tools/search",
		"LETTA_API_KEY":             "secret-value",
	}
	config, err := ConfigFromLookup(func(name string) string { return values[name] })
	if err != nil {
		t.Fatal(err)
	}
	if !config.Enabled || config.SearchMode != "hybrid" || config.MaxLimit != 50 || config.Timeout != 2*time.Second {
		t.Fatalf("config defaults = %+v", config)
	}
	if config.MaxResponseBytes != 256*1024 {
		t.Fatalf("response bound = %d", config.MaxResponseBytes)
	}
	if strings.Contains(fmt.Sprintf("%+v %#v", config, config), "secret-value") {
		t.Fatal("config debug representation disclosed bearer secret")
	}
}

func TestConfigFromLookupValidatesEnabledTransportAndBounds(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		values  map[string]string
		wantErr string
	}{
		{name: "invalid enabled flag", values: map[string]string{"LETTA_TOOL_SEARCH_ENABLED": "yes"}, wantErr: "LETTA_TOOL_SEARCH_ENABLED"},
		{name: "missing endpoint", values: map[string]string{"LETTA_TOOL_SEARCH_ENABLED": "true", "LETTA_API_KEY": "secret"}, wantErr: "LETTA_TOOL_SEARCH_URL"},
		{name: "missing key", values: map[string]string{"LETTA_TOOL_SEARCH_ENABLED": "true", "LETTA_TOOL_SEARCH_URL": "https://api.letta.example/v1/tools/search"}, wantErr: "LETTA_API_KEY"},
		{name: "remote plaintext", values: map[string]string{"LETTA_TOOL_SEARCH_ENABLED": "true", "LETTA_TOOL_SEARCH_URL": "http://letta.example/v1/tools/search", "LETTA_API_KEY": "secret"}, wantErr: "HTTPS"},
		{name: "loopback plaintext not opted in", values: map[string]string{"LETTA_TOOL_SEARCH_ENABLED": "true", "LETTA_TOOL_SEARCH_URL": "http://127.0.0.1:8283/v1/tools/search", "LETTA_API_KEY": "secret"}, wantErr: "ALLOW_INSECURE_LOOPBACK"},
		{name: "wrong endpoint path", values: map[string]string{"LETTA_TOOL_SEARCH_ENABLED": "true", "LETTA_TOOL_SEARCH_URL": "https://api.letta.example/v1/passages/search", "LETTA_API_KEY": "secret"}, wantErr: "/v1/tools/search"},
		{name: "endpoint query", values: map[string]string{"LETTA_TOOL_SEARCH_ENABLED": "true", "LETTA_TOOL_SEARCH_URL": "https://api.letta.example/v1/tools/search?token=bad", "LETTA_API_KEY": "secret"}, wantErr: "query"},
		{name: "invalid mode", values: map[string]string{"LETTA_TOOL_SEARCH_ENABLED": "true", "LETTA_TOOL_SEARCH_URL": "https://api.letta.example/v1/tools/search", "LETTA_API_KEY": "secret", "LETTA_TOOL_SEARCH_MODE": "magic"}, wantErr: "LETTA_TOOL_SEARCH_MODE"},
		{name: "oversized limit", values: map[string]string{"LETTA_TOOL_SEARCH_ENABLED": "true", "LETTA_TOOL_SEARCH_URL": "https://api.letta.example/v1/tools/search", "LETTA_API_KEY": "secret", "LETTA_TOOL_SEARCH_LIMIT": "101"}, wantErr: "LETTA_TOOL_SEARCH_LIMIT"},
		{name: "oversized timeout", values: map[string]string{"LETTA_TOOL_SEARCH_ENABLED": "true", "LETTA_TOOL_SEARCH_URL": "https://api.letta.example/v1/tools/search", "LETTA_API_KEY": "secret", "LETTA_TOOL_SEARCH_TIMEOUT": "11s"}, wantErr: "LETTA_TOOL_SEARCH_TIMEOUT"},
		{name: "invalid response bound", values: map[string]string{"LETTA_TOOL_SEARCH_ENABLED": "true", "LETTA_TOOL_SEARCH_URL": "https://api.letta.example/v1/tools/search", "LETTA_API_KEY": "secret", "LETTA_TOOL_SEARCH_MAX_RESPONSE_BYTES": "0"}, wantErr: "LETTA_TOOL_SEARCH_MAX_RESPONSE_BYTES"},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			_, err := ConfigFromLookup(func(name string) string { return test.values[name] })
			if err == nil || !strings.Contains(err.Error(), test.wantErr) {
				t.Fatalf("error = %v, want containing %q", err, test.wantErr)
			}
			if strings.Contains(fmt.Sprint(err), "secret") {
				t.Fatal("configuration error disclosed bearer secret")
			}
		})
	}
}

func TestClientSearchUsesBoundedOfficialContractWithoutRedirect(t *testing.T) {
	t.Parallel()

	var redirected bool
	target := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, request *http.Request) {
		redirected = true
		w.WriteHeader(http.StatusTeapot)
	}))
	defer target.Close()

	var source *httptest.Server
	source = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, request *http.Request) {
		if request.URL.Path != "/v1/tools/search" || request.Method != http.MethodPost {
			t.Fatalf("request = %s %s", request.Method, request.URL.Path)
		}
		if request.Header.Get("Authorization") != "Bearer secret-value" {
			t.Fatal("missing Letta bearer")
		}
		if request.Header.Get("Content-Type") != "application/json" {
			t.Fatalf("content-type = %q", request.Header.Get("Content-Type"))
		}
		body := make([]byte, request.ContentLength)
		_, _ = request.Body.Read(body)
		payload := string(body)
		for _, expected := range []string{`"query":"shipment status"`, `"limit":3`, `"search_mode":"hybrid"`} {
			if !strings.Contains(payload, expected) {
				t.Fatalf("body %s missing %s", payload, expected)
			}
		}
		w.Header().Set("Location", target.URL)
		w.WriteHeader(http.StatusTemporaryRedirect)
	}))
	defer source.Close()

	client, err := New(Config{
		Enabled: true, Endpoint: source.URL + "/v1/tools/search", APIKey: "secret-value",
		AllowInsecureLoopback: true, Timeout: time.Second, MaxResponseBytes: 1024, MaxLimit: 3, SearchMode: "hybrid",
	})
	if err != nil {
		t.Fatal(err)
	}
	_, err = client.Search(context.Background(), "shipment status", 50)
	if err == nil || !strings.Contains(err.Error(), "status 307") {
		t.Fatalf("redirect error = %v", err)
	}
	if redirected {
		t.Fatal("Letta client followed a redirect")
	}
	if strings.Contains(fmt.Sprintf("%#v", client), "secret-value") {
		t.Fatal("client debug representation disclosed bearer secret")
	}
}

func TestClientSearchDecodesOnlyBoundedToolNames(t *testing.T) {
	t.Parallel()

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`[
			{"combined_score":0.9,"tool":{"id":"remote-1","name":"track_shipment","source_code":"must not be trusted"}},
			{"combined_score":0.8,"tool":{"id":"remote-2","name":"book_shipment"}},
			{"combined_score":0.7,"tool":{"id":"remote-3","name":"track_shipment"}},
			{"combined_score":0.6,"tool":{"id":"remote-4","name":""}}
		]`))
	}))
	defer server.Close()

	client, err := New(Config{
		Enabled: true, Endpoint: server.URL + "/v1/tools/search", APIKey: "secret",
		AllowInsecureLoopback: true, Timeout: time.Second, MaxResponseBytes: 4096, MaxLimit: 10, SearchMode: "fts",
	})
	if err != nil {
		t.Fatal(err)
	}
	matches, err := client.Search(context.Background(), "shipping", 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(matches) != 2 || matches[0].Name != "track_shipment" || matches[1].Name != "book_shipment" {
		t.Fatalf("matches = %+v", matches)
	}
}

func TestClientSearchRejectsOversizedAndMalformedResponses(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		body string
	}{
		{name: "oversized", body: strings.Repeat("x", 257)},
		{name: "malformed", body: `{"not":"an array"}`},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				_, _ = w.Write([]byte(test.body))
			}))
			defer server.Close()
			client, err := New(Config{
				Enabled: true, Endpoint: server.URL + "/v1/tools/search", APIKey: "secret",
				AllowInsecureLoopback: true, Timeout: time.Second, MaxResponseBytes: 256, MaxLimit: 10, SearchMode: "vector",
			})
			if err != nil {
				t.Fatal(err)
			}
			if _, err = client.Search(context.Background(), "query", 10); err == nil {
				t.Fatal("unsafe response was accepted")
			}
		})
	}
}

func TestClientRejectsCredentialAndEndpointSmuggling(t *testing.T) {
	t.Parallel()

	base := Config{
		Enabled: true, Endpoint: "https://api.letta.example/v1/tools/search", APIKey: "secret",
		Timeout: time.Second, MaxResponseBytes: 1024, MaxLimit: 10, SearchMode: "hybrid",
	}
	tests := []struct {
		name   string
		mutate func(*Config)
	}{
		{name: "userinfo", mutate: func(config *Config) { config.Endpoint = "https://user@api.letta.example/v1/tools/search" }},
		{name: "fragment", mutate: func(config *Config) { config.Endpoint += "#fragment" }},
		{name: "unsupported scheme", mutate: func(config *Config) { config.Endpoint = "file:///v1/tools/search" }},
		{name: "header newline", mutate: func(config *Config) { config.APIKey = "secret\r\ninjected: true" }},
		{name: "negative timeout", mutate: func(config *Config) { config.Timeout = -time.Second }},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			config := base
			test.mutate(&config)
			if _, err := New(config); err == nil {
				t.Fatal("unsafe configuration was accepted")
			}
		})
	}
}

func TestClientRejectsNilAndOversizedQueriesWithoutNetwork(t *testing.T) {
	t.Parallel()

	var client *Client
	if got := client.String(); got != "lettatools.Client<nil>" {
		t.Fatalf("nil client string = %q", got)
	}
	if _, err := client.Search(context.Background(), "query", 1); err == nil {
		t.Fatal("nil client search succeeded")
	}

	server := httptest.NewServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		t.Fatal("oversized query reached the network")
	}))
	defer server.Close()
	client, err := New(Config{
		Enabled: true, Endpoint: server.URL + "/v1/tools/search", APIKey: "secret",
		AllowInsecureLoopback: true, Timeout: time.Second, MaxResponseBytes: 1024, MaxLimit: 10, SearchMode: "hybrid",
	})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := client.Search(context.Background(), strings.Repeat("q", 4097), 1); err == nil {
		t.Fatal("oversized query was accepted")
	}
}
