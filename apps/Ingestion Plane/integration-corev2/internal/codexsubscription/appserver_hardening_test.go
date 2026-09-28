package codexsubscription

import (
	"encoding/json"
	"errors"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"sync/atomic"
	"testing"
	"time"
)

func writeCodexConfig(t *testing.T, home, content string) {
	t.Helper()
	if err := os.MkdirAll(home, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(home, codexConfigFileName), []byte(content), 0o600); err != nil {
		t.Fatal(err)
	}
}

func TestConfiguredMCPServers(t *testing.T) {
	cases := []struct {
		name    string
		config  string
		want    []string
		refused bool
	}{
		{name: "no config"},
		{name: "unrelated", config: "model = \"gpt-5.6-terra\"\n[projects.\"/work\"]\ntrust_level = \"trusted\"\n"},
		{name: "codex-written tables", config: "[mcp_servers.docs]\ncommand = \"npx\"\nargs = [\"-y\", \"docs\"]\n\n[mcp_servers.docs.env]\nTOKEN_ENV = \"X\"\n\n[mcp_servers.\"search-1\"]\nurl = \"https://example.invalid/mcp\"\n[ mcp_servers . 'z_last' ] # trailing comment\nenabled = true\n", want: []string{"docs", "search-1", "z_last"}},
		{name: "comment and value mentions", config: "# [mcp_servers.old]\nnote = \"see mcp_servers docs\"\nbody = \"\"\"\n[mcp_servers.inside_string]\n\"\"\"\n", want: nil},
		// A delimiter inside a one-line literal string or a comment opens no
		// multi-line string, so the header after it is still found.
		{name: "triple quote in a literal string", config: "note = '\"\"\"'\n[mcp_servers.evil]\ncommand = \"/bin/sh\"\n", want: []string{"evil"}},
		{name: "triple quote in a comment", config: "model = \"x\" # \"\"\"\n[mcp_servers.evil]\ncommand = \"/bin/sh\"\n", want: []string{"evil"}},
		{name: "literal triple quote in a comment", config: "model = \"x\" # '''\n[mcp_servers.evil]\ncommand = \"/bin/sh\"\n", want: []string{"evil"}},
		{name: "inline table", config: "mcp_servers = { docs = { command = \"npx\" } }\n", want: []string{"docs"}},
		{name: "bare table", config: "[mcp_servers]\ndocs.command = \"npx\"\n", want: []string{"docs"}},
		{name: "dotted key", config: "mcp_servers.docs.command = \"npx\"\n", want: []string{"docs"}},
		{name: "escaped key", config: "[\"mcp\\u005Fservers\".docs]\ncommand = \"npx\"\n", want: []string{"docs"}},
		{name: "empty table", config: "[mcp_servers]\n", want: nil},
		{name: "array of tables", config: "[[mcp_servers.docs]]\ncommand = \"npx\"\n", refused: true},
		{name: "unaddressable name", config: "[mcp_servers.\"my.server\"]\ncommand = \"npx\"\n", refused: true},
		{name: "not a table", config: "mcp_servers = \"docs\"\n", refused: true},
		{name: "server not a table", config: "[mcp_servers]\ndocs = \"npx\"\n", refused: true},
		{name: "profile scoped", config: "[profiles.work.mcp_servers.docs]\ncommand = \"npx\"\n", refused: true},
		{name: "does not decode", config: "[mcp_servers.docs\ncommand = \"npx\"\n", refused: true},
		{name: "duplicate table", config: "[mcp_servers.docs]\ncommand = \"a\"\n[mcp_servers.docs]\ncommand = \"b\"\n", refused: true},
		{name: "hex escaped key", config: "[\"mcp\\x5Fservers\".docs]\ncommand = \"npx\"\n", refused: true},
		{name: "escaped value is fine", config: "greeting = \"caf\\u00E9\"\n"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			home := t.TempDir()
			if tc.config != "" {
				writeCodexConfig(t, home, tc.config)
			}
			got, err := configuredMCPServers(home)
			if tc.refused {
				if !errors.Is(err, errUnrecognizedMCPConfig) {
					t.Fatalf("error = %v, want errUnrecognizedMCPConfig", err)
				}
				return
			}
			if err != nil || !reflect.DeepEqual(got, tc.want) {
				t.Fatalf("got %q, %v; want %q", got, err, tc.want)
			}
		})
	}
}

func TestCodexProcessArgs(t *testing.T) {
	args, err := codexProcessArgs(true, "/data/conn-1/.ephemeral/abc", []string{"docs", "search-1"})
	if err != nil {
		t.Fatal(err)
	}
	base := codexAppServerArgs(true)
	want := append(append([]string{}, base[:len(base)-1]...),
		"-c", `sqlite_home="/data/conn-1/.ephemeral/abc"`,
		"-c", "mcp_servers.docs.enabled=false",
		"-c", "mcp_servers.search-1.enabled=false",
		"app-server")
	if !reflect.DeepEqual(args, want) {
		t.Fatalf("hardened args = %q\nwant %q", args, want)
	}
	if !reflect.DeepEqual(codexAppServerArgs(true), base) {
		t.Fatal("codexProcessArgs modified the shared hardened list")
	}
	legacy, err := codexProcessArgs(false, "/data/x", []string{"docs"})
	if err != nil || !reflect.DeepEqual(legacy, codexAppServerArgs(false)) {
		t.Fatalf("unhardened args = %q, %v; want exactly the legacy list", legacy, err)
	}
	if _, err := codexProcessArgs(true, "/data/\xff", nil); err == nil {
		t.Fatal("an invalid UTF-8 path was accepted")
	}
}

func TestTOMLBasicString(t *testing.T) {
	for raw, want := range map[string]string{
		`/data/conn-1/.ephemeral/abc`: `"/data/conn-1/.ephemeral/abc"`,
		`C:\Users\app\"x"`:            `"C:\\Users\\app\\\"x\""`,
		"tab\tnew\nline\x7f":          `"tab\u0009new\u000Aline\u007F"`,
		"/data/caf\u00e9":             "\"/data/caf\u00e9\"",
	} {
		got, err := tomlBasicString(raw)
		if err != nil || got != want {
			t.Fatalf("tomlBasicString(%q) = %q, %v; want %q", raw, got, err, want)
		}
	}
}

func TestNewManagerSweepsLeftoverEphemeralHomes(t *testing.T) {
	root := t.TempDir()
	leftover := filepath.Join(root, "conn-a", ephemeralDirName, "0f5c2d9e")
	if err := os.MkdirAll(leftover, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(leftover, "logs_2.sqlite"), []byte("prompt log"), 0o600); err != nil {
		t.Fatal(err)
	}
	auth := filepath.Join(root, "conn-a", "auth.json")
	if err := os.WriteFile(auth, []byte("{}"), 0o600); err != nil {
		t.Fatal(err)
	}
	other := filepath.Join(root, "conn-b", "workspace")
	if err := os.MkdirAll(other, 0o700); err != nil {
		t.Fatal(err)
	}
	if _, err := NewManager(Config{Enabled: true, Home: root}, &fakeRunner{}); err != nil {
		t.Fatalf("NewManager: %v", err)
	}
	if _, err := os.Stat(leftover); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("a leftover ephemeral state directory survived startup")
	}
	for _, kept := range []string{auth, other, filepath.Join(root, "conn-a", ephemeralDirName)} {
		if _, err := os.Stat(kept); err != nil {
			t.Fatalf("the sweep removed %s: %v", kept, err)
		}
	}
}

func TestManagerCloseEndsPendingLogins(t *testing.T) {
	runner := &fakeRunner{}
	manager, err := NewManager(Config{Enabled: true, Home: t.TempDir(), LoginTTL: time.Minute}, runner)
	if err != nil {
		t.Fatal(err)
	}
	login, err := manager.StartLogin(t.Context(), "conn-1")
	if err != nil {
		t.Fatal(err)
	}
	if err := manager.Close(); err != nil {
		t.Fatal(err)
	}
	if !runner.login.closed {
		t.Fatal("Close left a pending login process running")
	}
	if _, err := manager.PollLogin(t.Context(), login.LoginID); !errors.Is(err, ErrLoginNotFound) {
		t.Fatalf("PollLogin after Close = %v, want ErrLoginNotFound", err)
	}
	var nilManager *Manager
	if err := nilManager.Close(); err != nil {
		t.Fatal(err)
	}
}

func fakeAppServerRunner(t *testing.T, opts ...ProcessOption) *ProcessRunner {
	t.Helper()
	executable, err := os.Executable()
	if err != nil {
		t.Fatalf("locate test binary: %v", err)
	}
	t.Setenv(fakeAppServerEnv, "1")
	t.Setenv(fakeReportEnv, "1")
	return NewProcessRunner(executable, opts...)
}

func appServerRequest() InvokeRequest {
	return InvokeRequest{RequestID: "req-1", Model: "gpt-5.6-terra", Messages: []ChatMessage{{Role: "user", Content: "hello"}}, ReasoningEffort: "low"}
}

func TestAppServerDisablesConfiguredMCPServers(t *testing.T) {
	runner := fakeAppServerRunner(t)
	home := t.TempDir()
	if err := os.WriteFile(filepath.Join(home, "auth.json"), []byte("{}"), 0o600); err != nil {
		t.Fatal(err)
	}
	writeCodexConfig(t, home, "sqlite_home = \"/somewhere/else\"\n[mcp_servers.docs]\ncommand = \"npx\"\n")
	response, err := runner.Invoke(t.Context(), home, appServerRequest())
	if err != nil {
		t.Fatalf("Invoke: %v", err)
	}
	var report fakeAppServerReport
	if err := json.Unmarshal([]byte(response.Content), &report); err != nil {
		t.Fatalf("decode report: %v", err)
	}
	quoted, _ := tomlBasicString(report.SQLiteHome)
	subcommand := slices.Index(report.Args, "app-server")
	for _, flag := range []string{"sqlite_home=" + quoted, "mcp_servers.docs.enabled=false"} {
		index := slices.Index(report.Args, flag)
		if index < 1 || report.Args[index-1] != "-c" || index > subcommand {
			t.Fatalf("spawned args %q lack the root-level override %q", report.Args, flag)
		}
	}

	// A declaration the broker cannot disable refuses the inference process.
	refused := t.TempDir()
	if err := os.WriteFile(filepath.Join(refused, "auth.json"), []byte("{}"), 0o600); err != nil {
		t.Fatal(err)
	}
	writeCodexConfig(t, refused, "[mcp_servers.\"my.server\"]\ncommand = \"npx\"\n")
	if _, err := runner.Invoke(t.Context(), refused, appServerRequest()); !errors.Is(err, errUnrecognizedMCPConfig) {
		t.Fatalf("Invoke = %v, want errUnrecognizedMCPConfig", err)
	}
	if _, err := os.Stat(filepath.Join(refused, ephemeralDirName)); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("an app-server was spawned for an unrecognized MCP declaration")
	}
	// Logout starts no thread, so it still runs: a user can always disconnect.
	if err := runner.Logout(t.Context(), refused); err != nil {
		t.Fatalf("Logout with an unrecognized MCP declaration: %v", err)
	}

	// Unhardened, the pre-hardening argument list is used unchanged.
	legacy := fakeAppServerRunner(t, WithHardening(false))
	response, err = legacy.Invoke(t.Context(), home, appServerRequest())
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal([]byte(response.Content), &report); err != nil || !reflect.DeepEqual(report.Args, codexAppServerArgs(false)) {
		t.Fatalf("unhardened args = %q, %v", report.Args, err)
	}
}

func TestAppServerRefusesSpawnWhenCodexWouldRefresh(t *testing.T) {
	cases := []struct {
		name       string
		left       time.Duration
		skew       time.Duration
		status     int
		spawned    bool
		tokenCalls int
	}{
		// The broker's refresh fails and Codex would refresh the token during
		// the call by itself (90s call plus Codex's 5m window).
		{name: "transient failure inside the window", left: 5 * time.Minute, status: http.StatusServiceUnavailable, tokenCalls: 1},
		{name: "transient failure, expired token", left: -time.Minute, status: http.StatusServiceUnavailable, tokenCalls: 1},
		// The failed refresh leaves a token that outlives the window.
		{name: "transient failure outside the window", left: 8 * time.Minute, status: http.StatusServiceUnavailable, spawned: true, tokenCalls: 1},
		// A skew below the window still refreshes a token Codex would refresh.
		{name: "small skew", left: 3 * time.Minute, skew: time.Minute, status: http.StatusOK, spawned: true, tokenCalls: 1},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			now := time.Now().UTC().Truncate(time.Second)
			auth := newFakeAuth(t, func(int, url.Values) (int, string) {
				if tc.status != http.StatusOK {
					return tc.status, `{}`
				}
				return http.StatusOK, grantBody(rotatedAccess(time.Now(), "rotated"), "", "")
			})
			store := newCredentialStore(credentialStoreConfig{TokenURL: auth.URL + "/oauth/token", Skew: tc.skew, Now: func() time.Time { return now }})
			home := filepath.Join(t.TempDir(), "conn-1")
			writeAuthJSON(t, home, authOptions{accessExp: now.Add(tc.left)})
			runner := fakeAppServerRunner(t, WithCredentials(store))
			_, err := runner.Invoke(t.Context(), home, appServerRequest())
			_, statErr := os.Stat(filepath.Join(home, ephemeralDirName))
			spawned := statErr == nil
			if spawned != tc.spawned || auth.callCount() != tc.tokenCalls {
				t.Fatalf("spawned=%v token calls=%d err=%v, want spawned=%v and %d", spawned, auth.callCount(), err, tc.spawned, tc.tokenCalls)
			}
			if tc.spawned && err != nil {
				t.Fatalf("Invoke: %v", err)
			}
			var upstream *UpstreamError
			if !tc.spawned && (!errors.As(err, &upstream) || upstream.Kind != upstreamKindRefresh) {
				t.Fatalf("error = %v, want token_refresh_unavailable", err)
			}
		})
	}
}

// A refresh whose rotated grant cannot be written back leaves the consumed
// refresh token in auth.json, where a spawned Codex would find and reuse it
// (D7). No process may start until a later call's write succeeds.
func TestAppServerRefusesSpawnWhileWriteBackIsPending(t *testing.T) {
	now := time.Now().UTC().Truncate(time.Second)
	rotated, refresh2 := rotatedAccess(now, sentinel("access2")), sentinel("refresh2")
	auth := newFakeAuth(t, rotatingAuth(func(int) (int, string) {
		return http.StatusOK, grantBody(rotated, refresh2, "")
	}))
	store := newCredentialStore(credentialStoreConfig{TokenURL: auth.URL + "/oauth/token", Now: func() time.Time { return now }})
	var failWrites atomic.Bool
	failWrites.Store(true)
	store.writeFile = func(dir, path string, data []byte, mode os.FileMode) error {
		if failWrites.Load() {
			return errors.New("no space left on device")
		}
		return writeFileAtomic(dir, path, data, mode)
	}
	home := filepath.Join(t.TempDir(), "conn-1")
	fixture := writeAuthJSON(t, home, authOptions{accessExp: now.Add(time.Minute)})
	runner := fakeAppServerRunner(t, WithCredentials(store))
	ephemeral := filepath.Join(home, ephemeralDirName)

	_, err := runner.Invoke(t.Context(), home, appServerRequest())
	var upstream *UpstreamError
	if !errors.As(err, &upstream) || upstream.Kind != upstreamKindRefresh || upstream.Code != "write_back_pending" {
		t.Fatalf("Invoke = %v, want write_back_pending", err)
	}
	if _, statErr := os.Stat(ephemeral); !errors.Is(statErr, os.ErrNotExist) {
		t.Fatal("an app-server was spawned while auth.json held the consumed refresh token")
	}
	if pending, ok := store.unsavedFor(home); !ok || pending.grant.refresh != refresh2 {
		t.Fatal("the rotated grant is not kept in memory")
	}
	if disk := readDiskTokens(t, fixture.path); disk.Tokens.RefreshToken != fixture.refresh {
		t.Fatal("auth.json changed although the write failed")
	}

	// The next call writes the grant back first, then spawns without a
	// second refresh.
	failWrites.Store(false)
	if _, err := runner.Invoke(t.Context(), home, appServerRequest()); err != nil {
		t.Fatalf("Invoke after the disk recovered: %v", err)
	}
	if _, statErr := os.Stat(ephemeral); statErr != nil {
		t.Fatal("no app-server was spawned once auth.json held the rotated grant")
	}
	if disk := readDiskTokens(t, fixture.path); disk.Tokens.RefreshToken != refresh2 || auth.callCount() != 1 {
		t.Fatalf("rotated refresh on disk %v, token calls %d; want true and 1", disk.Tokens.RefreshToken == refresh2, auth.callCount())
	}
}
