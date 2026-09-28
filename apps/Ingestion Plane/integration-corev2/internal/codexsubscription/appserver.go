package codexsubscription

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/google/uuid"
	"github.com/rs/zerolog"
)

const (
	codexReadOnlySandbox  = "read-only"
	codexBaseInstructions = "You are Verevon's text-only assistant. Answer the supplied conversation directly. Do not execute tools, access files, run commands, or modify anything. When a supplied protocol requests structured tool proposals, return them as JSON data for Verevon to validate and execute. Otherwise return only the answer."
)

// ProcessRunner speaks the documented JSON-RPC-over-stdio Codex app-server
// protocol. It invokes the configured executable directly (never through a
// shell), so a configured command cannot become shell injection. It is the
// phase-1 Authenticator (device-code login and logout) and the optional
// app-server Transport fallback.
type ProcessRunner struct {
	command      string
	hardened     bool
	threadEffort bool
	creds        *credentialStore
	logger       zerolog.Logger
}

// ProcessOption configures a ProcessRunner.
type ProcessOption func(*ProcessRunner)

// WithCredentials lets the runner refresh the connection's token before it
// spawns an inference process, so Codex never refreshes during a call and
// the broker stays the only refresher.
func WithCredentials(store *credentialStore) ProcessOption {
	return func(r *ProcessRunner) { r.creds = store }
}

// WithHardening turns the process-level hardening flags on or off. The
// default is on; off restores the pre-hardening argument list as a
// no-rebuild escape hatch.
func WithHardening(hardened bool) ProcessOption {
	return func(r *ProcessRunner) { r.hardened = hardened }
}

// WithThreadEffort turns the thread/start reasoning-effort override on or
// off. The default is on; off restores the earlier thread/start params
// exactly as a no-rebuild escape hatch (see threadStartParams).
func WithThreadEffort(enabled bool) ProcessOption {
	return func(r *ProcessRunner) { r.threadEffort = enabled }
}

// WithLogger sets the content-free logger.
func WithLogger(logger zerolog.Logger) ProcessOption {
	return func(r *ProcessRunner) { r.logger = logger }
}

func NewProcessRunner(command string, opts ...ProcessOption) *ProcessRunner {
	runner := &ProcessRunner{command: strings.TrimSpace(command), hardened: true, threadEffort: true, logger: zerolog.Nop()}
	for _, opt := range opts {
		opt(runner)
	}
	return runner
}

// Kind reports the app-server transport.
func (r *ProcessRunner) Kind() TransportKind { return TransportAppServer }

func (r *ProcessRunner) BeginDeviceLogin(ctx context.Context, codeHome string) (LoginProcess, DeviceCode, error) {
	client, err := r.start(ctx, codeHome)
	if err != nil {
		return nil, DeviceCode{}, err
	}
	var result struct {
		VerificationURL string `json:"verificationUrl"`
		UserCode        string `json:"userCode"`
	}
	if err := client.call(ctx, "account/login/start", map[string]string{"type": "chatgptDeviceCode"}, &result); err != nil {
		_ = client.Close()
		return nil, DeviceCode{}, err
	}
	if result.VerificationURL == "" || result.UserCode == "" {
		_ = client.Close()
		return nil, DeviceCode{}, errors.New("codex app-server returned an incomplete device code")
	}
	return &processLogin{client: client, codeHome: codeHome}, DeviceCode{VerificationURL: result.VerificationURL, UserCode: result.UserCode}, nil
}

func (r *ProcessRunner) Invoke(ctx context.Context, codeHome string, request InvokeRequest) (InvokeResponse, error) {
	return r.invoke(ctx, codeHome, request, nil)
}

func (r *ProcessRunner) InvokeStream(ctx context.Context, codeHome string, request InvokeRequest, onDelta func(string) error) (InvokeResponse, error) {
	return r.invoke(ctx, codeHome, request, onDelta)
}

func (r *ProcessRunner) invoke(ctx context.Context, codeHome string, request InvokeRequest, onDelta func(string) error) (response InvokeResponse, err error) {
	defer func() {
		if err != nil {
			r.logFailedCall(request, err)
		}
	}()
	tokens, err := r.ensureCredential(ctx, codeHome)
	if err != nil {
		return InvokeResponse{}, err
	}
	phases := make(map[string]int64, 6)
	spawnStarted := time.Now()
	client, err := r.spawn(codeHome, true)
	if err != nil {
		return InvokeResponse{}, err
	}
	defer client.Close()
	initializeStarted := recordPhase(phases, PhaseSpawn, spawnStarted)
	if err := client.initialize(ctx); err != nil {
		return InvokeResponse{}, err
	}
	recordPhase(phases, PhaseInitialize, initializeStarted)

	// ensureCredential already rejected a missing local login and refreshed
	// a token near expiry. Avoid an account/read RPC on every answer:
	// thread/start and turn/start are the authoritative authenticated
	// operations.

	workspace := filepath.Join(codeHome, "workspace")
	if err := os.MkdirAll(workspace, 0o700); err != nil {
		return InvokeResponse{}, fmt.Errorf("create isolated codex workspace: %w", err)
	}
	var thread struct {
		Model           string  `json:"model"`
		ModelProvider   string  `json:"modelProvider"`
		ReasoningEffort *string `json:"reasoningEffort"`
		Thread          struct {
			ID string `json:"id"`
		} `json:"thread"`
	}
	threadStarted := time.Now()
	if err := client.call(ctx, "thread/start", threadStartParams(workspace, request, r.threadEffort), &thread); err != nil {
		return InvokeResponse{}, fmt.Errorf("start Codex subscription thread: %w", err)
	}
	recordPhase(phases, PhaseThreadStart, threadStarted)
	if strings.TrimSpace(thread.Thread.ID) == "" {
		return InvokeResponse{}, errors.New("codex app-server did not return a thread id")
	}
	if err := validateServingModel(request.Model, thread.Model, thread.ModelProvider); err != nil {
		return InvokeResponse{}, err
	}
	if r.threadEffort {
		// Without the override the thread keeps the model's own effort, so a
		// difference is expected and not worth a warning on every call.
		r.checkThreadEffort(request.ReasoningEffort, thread.ReasoningEffort)
	}
	var turn struct {
		Turn struct {
			ID string `json:"id"`
		} `json:"turn"`
	}
	turnStarted := time.Now()
	if err := client.call(ctx, "turn/start", turnStartParams(thread.Thread.ID, request), &turn); err != nil {
		return InvokeResponse{}, fmt.Errorf("start Codex subscription turn: %w", err)
	}
	turnAccepted := recordPhase(phases, PhaseTurnStart, turnStarted)
	completed, err := client.waitForTurn(ctx, thread.Thread.ID, turn.Turn.ID, onDelta)
	if err != nil {
		return InvokeResponse{}, r.resolveTurnFailure(ctx, client, codeHome, tokens, err)
	}
	recordPhase(phases, PhaseTurnTotal, turnAccepted)
	if !completed.firstText.IsZero() {
		phases[PhaseFirstDelta] = elapsedMs(turnAccepted, completed.firstText)
	}
	if strings.TrimSpace(completed.content) == "" {
		return InvokeResponse{}, errors.New("codex subscription turn completed without text")
	}
	return InvokeResponse{
		RequestID: request.RequestID,
		Content:   completed.content,
		ModelUsed: thread.Model,
		Usage:     completed.usage,
		PhasesMs:  phases,
	}, nil
}

// codexRefreshWindow is Codex's own proactive refresh window
// (CHATGPT_ACCESS_TOKEN_REFRESH_WINDOW_MINUTES in the pinned
// login/src/auth/manager.rs). A spawned Codex refreshes a token that expires
// inside it by itself, holding only its in-process lock.
const codexRefreshWindow = 5 * time.Minute

// ensureCredential runs before any inference process is spawned. With a
// credential store it makes sure the token outlives the call plus Codex's
// refresh window, refreshing it first when needed, so the spawned Codex never
// refreshes and the broker stays the only refresher (D7). A login that needs
// reauthentication spawns nothing, and neither does a token the broker could
// not refresh that Codex would refresh during the call, nor a rotated grant
// not yet written back to auth.json: two refreshes of one refresh token
// revoke the whole token family. Without a store it keeps the plain presence
// check. It returns the token set the spawned Codex will read, so a turn
// Codex fails as unauthorized can run the broker's one forced recovery.
func (r *ProcessRunner) ensureCredential(ctx context.Context, codeHome string) (tokenSet, error) {
	if r.creds == nil {
		ready, err := persistedAuthReady(codeHome)
		if err != nil {
			return tokenSet{}, err
		}
		if !ready {
			return tokenSet{}, ErrReauthenticationRequired
		}
		return tokenSet{}, nil
	}
	horizon := codexRefreshHorizon(ctx, r.creds.now())
	tokens, _, err := r.creds.current(ctx, codeHome, horizon)
	if err == nil && !tokens.AccessExpiry.IsZero() && !tokens.AccessExpiry.After(horizon) {
		// Even a fresh grant expires inside the window: Codex would refresh.
		err = &UpstreamError{Kind: upstreamKindRefresh, Code: "token_expired"}
	}
	if err == nil {
		if _, pending := r.creds.unsavedFor(codeHome); pending {
			// The rotated grant could not be written back, so it lives only in
			// memory while auth.json still holds the refresh token the endpoint
			// already consumed. Codex reads that file, not this memory, and a
			// refresh with the consumed token revokes the whole sign-in (D7).
			// Every call retries the write; until one succeeds, spawn nothing.
			r.logger.Warn().Str("event", "refresh_write_back_pending").Msg("codex_subscription_credentials")
			return tokenSet{}, &UpstreamError{Kind: upstreamKindRefresh, Code: "write_back_pending", Retryable: true}
		}
	}
	switch {
	case err == nil:
		return tokens, nil
	case errors.Is(err, ErrReauthenticationRequired):
		return tokenSet{}, ErrReauthenticationRequired
	case ctx.Err() != nil:
		return tokenSet{}, fmt.Errorf("refresh Codex subscription credential: %w", ctx.Err())
	}
	kind, code := errorLogFields(err)
	r.logger.Warn().Str("event", "refresh_before_spawn_failed").Str("error_kind", kind).Str("error_code", code).Msg("codex_subscription_credentials")
	return tokenSet{}, err
}

// rateLimitsReadTimeout bounds the one account/rateLimits/read a failed
// usage-limit turn may send. A variable only so a test can shorten it.
var rateLimitsReadTimeout = 3 * time.Second

// resolveTurnFailure completes a failed turn's typed error while the process
// is still open. A usage limit whose reset no rate-limit notification gave is
// looked up once with account/rateLimits/read, a non-model GET of the
// account's usage; a read failure or timeout leaves the reset unknown. A turn
// Codex failed as unauthorized gets the broker's one forced credential
// recovery: a recovery that confirms reauthentication returns
// ErrReauthenticationRequired, so the connection is marked needs_refresh, and
// a recovery that succeeds leaves a retryable failure, so a transient 401
// never disconnects the user.
func (r *ProcessRunner) resolveTurnFailure(ctx context.Context, client *appServerClient, codeHome string, used tokenSet, err error) error {
	var limited *RateLimitError
	if errors.As(err, &limited) && limited.Kind == RateLimitUsage && limited.ResetsAt.IsZero() {
		readUsageReset(ctx, client, limited, time.Now())
		return err
	}
	var upstream *UpstreamError
	if !errors.As(err, &upstream) || upstream.Kind != upstreamKindTurnFailed || upstream.Code != "unauthorized" || r.creds == nil {
		return err
	}
	// Codex's own refresh already failed. End the process first, so no Codex
	// process reads the credential file while the broker refreshes it.
	_ = client.Close()
	if _, recoverErr := r.creds.recoverUnauthorized(ctx, codeHome, used); recoverErr != nil {
		return recoverErr
	}
	upstream.Retryable = true
	return upstream
}

// readUsageReset fills a usage limit's reset from account/rateLimits/read:
// the bucket a notification named first, then the default rateLimits
// snapshot, by the same exhausted-window rule as the notification. The plan
// type in the response is never read.
func readUsageReset(ctx context.Context, client *appServerClient, limited *RateLimitError, now time.Time) {
	readCtx, cancel := context.WithTimeout(ctx, rateLimitsReadTimeout)
	defer cancel()
	var response struct {
		RateLimits          *codexRateLimitSnapshot           `json:"rateLimits"`
		RateLimitsByLimitID map[string]codexRateLimitSnapshot `json:"rateLimitsByLimitId"`
	}
	if client.call(readCtx, "account/rateLimits/read", codexEmptyObjectParams(), &response) != nil {
		return
	}
	var candidates []codexRateLimitSnapshot
	if bucket, ok := response.RateLimitsByLimitID[limited.limitID]; ok && limited.limitID != "" {
		candidates = append(candidates, bucket)
	}
	if response.RateLimits != nil {
		candidates = append(candidates, *response.RateLimits)
	}
	for _, snapshot := range candidates {
		if limited.LimitReachedType == "" {
			limited.LimitReachedType = snapshot.limitReachedType()
		}
		if limited.applyExhaustedWindow(snapshot, now) {
			limited.resetSource = "read"
			return
		}
	}
}

// logFailedCall writes the one content-free line of a failed app-server
// call: typed kinds, sanitized codes, counts and account metadata only.
func (r *ProcessRunner) logFailedCall(request InvokeRequest, err error) {
	kind, code := errorLogFields(err)
	failure := ClassifyFailure(err, time.Now())
	httpStatus, retries, retryStatus, source := 0, 0, 0, "none"
	var upstream *UpstreamError
	if errors.As(err, &upstream) {
		httpStatus, retries, retryStatus = upstream.HTTPStatus, upstream.upstreamRetries, upstream.retryHTTPStatus
	}
	var limited *RateLimitError
	if errors.As(err, &limited) && limited.resetSource != "" {
		source = limited.resetSource
	}
	r.logger.Warn().
		Str("request_id", sanitizeUpstreamRequestID(request.RequestID)).
		Str("connection_id", sanitizeUpstreamRequestID(request.ConnectionID)).
		Str("transport", "app_server").
		Str("error_kind", kind).
		Str("error_code", code).
		Int("http_status", httpStatus).
		Int("upstream_retries", retries).
		Int("retry_http_status", retryStatus).
		Str("reason", string(failure.Code)).
		Str("resets_at", failure.FormatResetsAt()).
		Int("window_mins", failure.WindowMinutes).
		Str("limit_reached_type", failure.LimitReachedType).
		Str("rate_limits_source", source).
		Msg("codex_subscription_upstream")
}

// codexRefreshHorizon is the end of the call (its deadline, else the default
// invocation timeout from now) plus Codex's refresh window.
func codexRefreshHorizon(ctx context.Context, now time.Time) time.Time {
	end := now.Add(defaultInvocationTimeout)
	if deadline, ok := ctx.Deadline(); ok {
		end = deadline
	}
	return end.Add(codexRefreshWindow)
}

// recordPhase stores the whole milliseconds since started under phase and
// returns the moment it measured, so the next phase can begin from there.
func recordPhase(phases map[string]int64, phase string, started time.Time) time.Time {
	now := time.Now()
	phases[phase] = elapsedMs(started, now)
	return now
}

// elapsedMs is the monotonic distance between two readings in whole
// milliseconds, never negative.
func elapsedMs(from, to time.Time) int64 {
	return max(to.Sub(from).Milliseconds(), 0)
}

// checkThreadEffort logs, content-free, when the thread did not take the
// requested reasoning effort, so the startup prewarm cannot match the turn.
// The turn still runs: turn/start sets the effort of the model request itself.
func (r *ProcessRunner) checkThreadEffort(requested string, reported *string) {
	if requested == "" || (reported != nil && *reported == requested) {
		return
	}
	threadEffort := "none"
	if reported != nil {
		threadEffort = sanitizeEffortLabel(*reported)
	}
	r.logger.Warn().Str("event", "thread_effort_differs").Str("requested_effort", sanitizeEffortLabel(requested)).Str("thread_effort", threadEffort).Msg("codex_subscription_app_server")
}

// sanitizeEffortLabel keeps an effort name only when it is a short lowercase
// word, so a log line never carries arbitrary upstream text.
func sanitizeEffortLabel(effort string) string {
	if effort == "" || len(effort) > 16 {
		return "unrecognized"
	}
	for _, char := range effort {
		if char < 'a' || char > 'z' {
			return "unrecognized"
		}
	}
	return effort
}

func validateServingModel(requested, model, provider string) error {
	if model != requested || provider != "openai" {
		return errors.New("codex app-server did not preserve the selected subscription model and provider")
	}
	return nil
}

// threadStartParams starts the thread with the model, service tier and
// reasoning effort its one turn will use. Codex opens a startup-prewarm
// websocket at thread/start and sends the thread's settings as a generate=false
// request; the turn's first request continues from it only when every
// non-input request property matches (core client.rs
// responses_request_properties_match), otherwise Codex logs "websocket reuse
// properties didn't match" and sends the full request. The pinned
// ThreadStartParams has typed model and serviceTier fields but no effort field,
// so effort goes through the config override that turn/start's effort would
// otherwise replace: the model request's effort is unchanged, and only the
// prewarm stops defaulting to the model's own effort (medium for
// gpt-5.6-terra). A turn with an outputSchema still differs from the prewarm in
// its text format, which thread/start cannot carry in 0.153.4 (the prewarm turn
// context is built with no final output schema). withEffort=false
// (CODEX_APP_SERVER_THREAD_EFFORT=false) leaves the override out, restoring
// the earlier params exactly; turn/start still sets the effort.
func threadStartParams(workspace string, request InvokeRequest, withEffort bool) map[string]any {
	// Tool proposals are JSON data for Model Gateway. The broker must not
	// independently execute a shell or perform web searches for that data.
	config := map[string]any{"features.shell_tool": false, "features.unified_exec": false, "web_search": "disabled", "forced_login_method": "chatgpt"}
	if withEffort && request.ReasoningEffort != "" {
		config["model_reasoning_effort"] = request.ReasoningEffort
	}
	params := map[string]any{
		"cwd":              workspace,
		"model":            request.Model,
		"modelProvider":    "openai",
		"approvalPolicy":   "never",
		"sandbox":          codexReadOnlySandbox,
		"serviceName":      "coresystem_integration_core",
		"baseInstructions": codexBaseInstructions,
		"ephemeral":        true,
		"config":           config,
	}
	if request.ServiceTier != "" {
		params["serviceTier"] = request.ServiceTier
	}
	return params
}

func turnStartParams(threadID string, request InvokeRequest) map[string]any {
	params := map[string]any{
		"threadId": threadID,
		"effort":   request.ReasoningEffort,
		"input": []map[string]string{{
			"type": "text",
			"text": renderPrompt(request.Messages),
		}},
	}
	if request.ServiceTier != "" {
		params["serviceTier"] = request.ServiceTier
	}
	if len(request.OutputSchema) > 0 {
		params["outputSchema"] = request.OutputSchema
	}
	return params
}

func (r *ProcessRunner) Logout(ctx context.Context, codeHome string) error {
	ready, err := persistedAuthReady(codeHome)
	if err != nil {
		return err
	}
	if !ready {
		// A missing local credential is already logged out. Keeping this path
		// idempotent lets a stale database connection be disconnected cleanly.
		return nil
	}
	client, err := r.start(ctx, codeHome)
	if err != nil {
		return err
	}
	defer client.Close()
	var ignored json.RawMessage
	if err := client.call(ctx, "account/logout", codexEmptyObjectParams(), &ignored); err != nil {
		return fmt.Errorf("logout Codex subscription: %w", err)
	}
	return nil
}

func (r *ProcessRunner) start(ctx context.Context, codeHome string) (*appServerClient, error) {
	client, err := r.spawn(codeHome, false)
	if err != nil {
		return nil, err
	}
	if err := client.initialize(ctx); err != nil {
		return nil, err
	}
	return client, nil
}

// spawn starts the app-server process without the protocol handshake. Split
// from initialize so an invocation can time the two separately. forTurn marks
// an inference process: hardened, it is refused when config.toml declares MCP
// servers the broker cannot turn off, because that process starts a thread
// with approvals disabled. Login and logout start no thread and so no MCP
// server; they only get the overrides the broker could build.
func (r *ProcessRunner) spawn(codeHome string, forTurn bool) (*appServerClient, error) {
	if strings.TrimSpace(r.command) == "" {
		return nil, errors.New("CODEX_APP_SERVER_COMMAND is required")
	}
	var mcpServers []string
	if r.hardened {
		names, err := configuredMCPServers(codeHome)
		switch {
		case err == nil:
			mcpServers = names
		case forTurn:
			r.logger.Warn().Str("event", "hardening_refused").Str("error_code", "config_mcp_servers").Msg("codex_subscription_app_server")
			return nil, err
		}
	}
	// A device-code login intentionally outlives its initiating HTTP request.
	// Invocation callers still close this process when their timeout elapses.
	// Do not bind the process lifetime to the supplied context here.
	sqliteHome, err := createEphemeralSQLiteHome(codeHome)
	if err != nil {
		return nil, err
	}
	args, err := codexProcessArgs(r.hardened, sqliteHome, mcpServers)
	if err != nil {
		_ = os.RemoveAll(sqliteHome)
		return nil, fmt.Errorf("build Codex app-server arguments: %w", err)
	}
	cmd := exec.Command(r.command, args...) // #nosec G204 -- command is trusted deployment config, not user input.
	cmd.Env = codexEnvironment(codeHome, sqliteHome, npmLauncherPackageRoot(r.command))
	configureProcessGroup(cmd)
	stdin, err := cmd.StdinPipe()
	if err != nil {
		_ = os.RemoveAll(sqliteHome)
		return nil, fmt.Errorf("create Codex app-server stdin: %w", err)
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		_ = os.RemoveAll(sqliteHome)
		return nil, fmt.Errorf("create Codex app-server stdout: %w", err)
	}
	// Stderr may contain provider-side diagnostic text. Do not merge it into the
	// JSON-RPC channel or application logs, which could accidentally retain a
	// prompt or account-related diagnostic.
	cmd.Stderr = io.Discard
	if err := cmd.Start(); err != nil {
		_ = os.RemoveAll(sqliteHome)
		return nil, fmt.Errorf("start codex app-server: %w", err)
	}
	client := newAppServerClient(cmd, stdin, stdout)
	client.ephemeralDir = sqliteHome
	return client, nil
}

// ephemeralDirName holds one throwaway SQLite home per spawned process.
const ephemeralDirName = ".ephemeral"

// createEphemeralSQLiteHome makes <codeHome>/.ephemeral/<uuid> (0700). Codex's
// state and log databases (logs_2.sqlite, state_*.sqlite) land there instead
// of the connection home, and Close removes the directory, so no prompt log
// survives the process.
func createEphemeralSQLiteHome(codeHome string) (string, error) {
	parent := filepath.Join(codeHome, ephemeralDirName)
	if err := os.MkdirAll(parent, 0o700); err != nil {
		return "", fmt.Errorf("create ephemeral Codex state directory: %w", err)
	}
	dir := filepath.Join(parent, uuid.NewString())
	if err := os.Mkdir(dir, 0o700); err != nil {
		return "", fmt.Errorf("create ephemeral Codex state directory: %w", err)
	}
	return dir, nil
}

// initialize performs the app-server handshake, closing the process when it
// fails.
func (c *appServerClient) initialize(ctx context.Context) error {
	initCtx, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	var initialized json.RawMessage
	if err := c.call(initCtx, "initialize", map[string]any{
		"clientInfo": map[string]string{"name": "coresystem_integration_core", "title": "CoreSystem Integration Core", "version": "1"},
	}, &initialized); err != nil {
		_ = c.Close()
		return fmt.Errorf("initialize codex app-server: %w", err)
	}
	if err := c.notify("initialized", map[string]any{}); err != nil {
		_ = c.Close()
		return fmt.Errorf("notify codex app-server initialized: %w", err)
	}
	return nil
}

// codexAppServerArgs is the app-server argument list. The broker runs in a
// headless container backed by a persistent CODEX_HOME, so it always forces
// the documented file store: device login must not silently choose an
// unavailable desktop keyring and leave the database ahead of durable auth.
//
// Hardened (the default), it also turns off, at process level, everything a
// ChatGPT account could otherwise bring into a turn that runs with approvals
// disabled: plugins, remote plugins, apps and connectors, skills, hooks,
// tool suggestion, shell and exec tools, web search, memories, goals,
// multi-agent, image generation, analytics, feedback, history and update
// checks. Every key was checked against the pinned Codex 0.153.4 source;
// --strict-config makes Codex reject a key a future release renames instead
// of ignoring it. hardened=false restores the pre-hardening list.
// codexProcessArgs adds the per-process part: sqlite_home and a disable for
// each MCP server the connection's config.toml declares.
func codexAppServerArgs(hardened bool) []string {
	if !hardened {
		return []string{"app-server", "-c", `cli_auth_credentials_store="file"`}
	}
	return []string{
		"--strict-config",
		"-c", `cli_auth_credentials_store="file"`,
		"-c", "features.plugins=false",
		"-c", "features.remote_plugin=false",
		"-c", "features.apps=false",
		"-c", "features.hooks=false",
		"-c", "features.tool_suggest=false",
		"-c", "features.skill_search=false",
		"-c", "features.skill_mcp_dependency_install=false",
		"-c", "features.goals=false",
		"-c", "features.multi_agent=false",
		"-c", "features.image_generation=false",
		"-c", "features.shell_snapshot=false",
		"-c", "features.memories=false",
		"-c", "features.shell_tool=false",
		"-c", "features.unified_exec=false",
		"-c", "apps._default.enabled=false",
		"-c", "include_apps_instructions=false",
		"-c", "skills.bundled.enabled=false",
		"-c", "skills.include_instructions=false",
		"-c", "analytics.enabled=false",
		"-c", "feedback.enabled=false",
		"-c", `history.persistence="none"`,
		"-c", "check_for_update_on_startup=false",
		"-c", `web_search="disabled"`,
		"app-server",
	}
}

func codexEmptyObjectParams() map[string]any {
	return map[string]any{}
}

// codexEnvironment is the process environment with this connection's
// CODEX_HOME and, when set, its ephemeral CODEX_SQLITE_HOME. A non-empty
// npmPackageRoot means the command is the vendored native binary of that npm
// package: the environment then carries exactly the variables the npm launcher
// would have added (see appserver_launcher.go), replacing inherited ones. The
// broker's own secrets never reach a Codex process (see brokerSecretEnv).
func codexEnvironment(codeHome, sqliteHome, npmPackageRoot string) []string {
	scrubbed := []string{"CODEX_HOME=", "CODEX_SQLITE_HOME="}
	if npmPackageRoot != "" {
		for _, key := range codexManagedEnvKeys {
			scrubbed = append(scrubbed, key+"=")
		}
	}
	environment := make([]string, 0, len(os.Environ())+4)
	for _, entry := range os.Environ() {
		upper := strings.ToUpper(entry)
		if slices.ContainsFunc(scrubbed, func(prefix string) bool { return strings.HasPrefix(upper, prefix) }) {
			continue
		}
		if name, _, _ := strings.Cut(upper, "="); brokerSecretEnv(name) {
			continue
		}
		environment = append(environment, entry)
	}
	environment = append(environment, "CODEX_HOME="+codeHome)
	if sqliteHome != "" {
		environment = append(environment, "CODEX_SQLITE_HOME="+sqliteHome)
	}
	if npmPackageRoot != "" {
		environment = append(environment, codexManagedPackageRootEnv+"="+npmPackageRoot, codexManagedByNPMEnv+"=1")
	}
	return environment
}

// brokerSecretWords mark a variable as a credential when one of them is a
// whole underscore-separated word of its name. The broker's API keys, service
// tokens, OAuth client secrets, passwords and encryption keys are all named
// this way (TOKKEN is a misspelling some provider settings carry).
var brokerSecretWords = map[string]struct{}{
	"KEY": {}, "KEYS": {}, "SECRET": {}, "SECRETS": {}, "PASSWORD": {}, "PASSWD": {},
	"TOKEN": {}, "TOKENS": {}, "TOKKEN": {}, "CREDENTIAL": {}, "CREDENTIALS": {}, "DSN": {},
}

// brokerSecretEnv reports whether an upper-case variable name holds one of the
// broker's secrets, which a model-driven Codex process, and anything it
// starts, must not inherit: a database URL, which embeds its password, or a
// name with a word from brokerSecretWords. OPENAI_API_KEY and CODEX_API_KEY,
// which would also change how Codex signs in, are among them. PATH, HOME, the
// locale, TZ, TMPDIR, SSL_CERT_*, the proxy variables and the CODEX_* variables
// the broker sets pass through.
func brokerSecretEnv(name string) bool {
	if name == "DATABASE_URL" || strings.HasSuffix(name, "_DATABASE_URL") {
		return true
	}
	for _, word := range strings.Split(name, "_") {
		if _, secret := brokerSecretWords[word]; secret {
			return true
		}
	}
	return false
}

type processLogin struct {
	client   *appServerClient
	codeHome string
}

func (p *processLogin) Poll(_ context.Context) (ProcessLoginStatus, error) {
	for {
		select {
		case event, open := <-p.client.events:
			if !open {
				return ProcessLoginStatus{Status: "failed", ErrorCode: "app_server_closed", Message: "The Codex sign-in process ended."}, nil
			}
			switch event.Method {
			case "account/login/completed":
				status := parseLoginCompletion(event.Params)
				if status.Status != "connected" {
					return status, nil
				}
				if err := waitForPersistedAuth(p.codeHome, 2*time.Second); err != nil {
					return ProcessLoginStatus{
						Status:    "failed",
						ErrorCode: "auth_not_persisted",
						Message:   "ChatGPT sign-in completed, but its credential could not be saved. Start a new connection.",
					}, nil
				}
				return status, nil
			case "account/login/failed":
				return ProcessLoginStatus{Status: "failed", ErrorCode: "login_failed", Message: "ChatGPT sign-in did not complete."}, nil
			}
		default:
			return ProcessLoginStatus{Status: "pending"}, nil
		}
	}
}

func persistedAuthReady(codeHome string) (bool, error) {
	info, err := os.Stat(filepath.Join(codeHome, "auth.json"))
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	}
	if err != nil {
		return false, fmt.Errorf("inspect persisted Codex authentication: %w", err)
	}
	return info.Mode().IsRegular() && info.Size() > 0, nil
}

func waitForPersistedAuth(codeHome string, timeout time.Duration) error {
	deadline := time.Now().Add(timeout)
	for {
		ready, err := persistedAuthReady(codeHome)
		if err != nil {
			return err
		}
		if ready {
			return nil
		}
		if !time.Now().Before(deadline) {
			return ErrReauthenticationRequired
		}
		time.Sleep(50 * time.Millisecond)
	}
}

func (p *processLogin) Close() error { return p.client.Close() }

func parseLoginCompletion(params json.RawMessage) ProcessLoginStatus {
	var body struct {
		Success bool `json:"success"`
		Error   *struct {
			Code    string `json:"code"`
			Message string `json:"message"`
		} `json:"error"`
	}
	_ = json.Unmarshal(params, &body)
	if body.Error != nil {
		return ProcessLoginStatus{Status: "failed", ErrorCode: body.Error.Code, Message: body.Error.Message}
	}
	// The documented notification is emitted only after the selected login flow
	// completes. Older app-server versions omit success, so false is not treated
	// as failure unless they supplied a structured error.
	return ProcessLoginStatus{Status: "connected"}
}

type rpcNotification struct {
	Method string          `json:"method"`
	Params json.RawMessage `json:"params"`
}

type rpcResponse struct {
	ID     json.RawMessage `json:"id"`
	Result json.RawMessage `json:"result"`
	Error  *struct {
		Code    int    `json:"code"`
		Message string `json:"message"`
	} `json:"error"`
}

type appServerClient struct {
	cmd    *exec.Cmd
	stdin  io.WriteCloser
	writer *bufio.Writer

	mu      sync.Mutex
	nextID  int64
	pending map[int64]chan rpcResponse
	events  chan rpcNotification
	done    chan struct{}
	close   sync.Once

	// ephemeralDir is the process's throwaway CODEX_SQLITE_HOME, removed
	// once the process has exited.
	ephemeralDir string
}

func newAppServerClient(cmd *exec.Cmd, stdin io.WriteCloser, stdout io.Reader) *appServerClient {
	client := &appServerClient{
		cmd:     cmd,
		stdin:   stdin,
		writer:  bufio.NewWriter(stdin),
		nextID:  1,
		pending: make(map[int64]chan rpcResponse),
		events:  make(chan rpcNotification, 4_096),
		done:    make(chan struct{}),
	}
	go client.read(stdout)
	return client
}

func (c *appServerClient) read(stdout io.Reader) {
	scanner := bufio.NewScanner(stdout)
	scanner.Buffer(make([]byte, 64*1024), 4*1024*1024)
	for scanner.Scan() {
		line := scanner.Bytes()
		var envelope struct {
			ID     json.RawMessage `json:"id"`
			Method string          `json:"method"`
			Params json.RawMessage `json:"params"`
			Result json.RawMessage `json:"result"`
			Error  *struct {
				Code    int    `json:"code"`
				Message string `json:"message"`
			} `json:"error"`
		}
		if err := json.Unmarshal(line, &envelope); err != nil {
			continue
		}
		if len(envelope.ID) > 0 && string(envelope.ID) != "null" {
			var id int64
			if json.Unmarshal(envelope.ID, &id) == nil {
				c.mu.Lock()
				ch := c.pending[id]
				delete(c.pending, id)
				c.mu.Unlock()
				if ch != nil {
					ch <- rpcResponse{ID: envelope.ID, Result: envelope.Result, Error: envelope.Error}
					close(ch)
				}
			}
			continue
		}
		if envelope.Method != "" {
			select {
			case c.events <- rpcNotification{Method: envelope.Method, Params: envelope.Params}:
			default:
				// A slow client must not block the app-server reader. Turn completion
				// is still observable through the final notification in normal use.
			}
		}
	}
	c.failPending()
	close(c.events)
	close(c.done)
}

func (c *appServerClient) failPending() {
	c.mu.Lock()
	defer c.mu.Unlock()
	for id, ch := range c.pending {
		delete(c.pending, id)
		ch <- rpcResponse{Error: &struct {
			Code    int    `json:"code"`
			Message string `json:"message"`
		}{Code: -32000, Message: "codex app-server closed"}}
		close(ch)
	}
}

func (c *appServerClient) call(ctx context.Context, method string, params any, target any) error {
	c.mu.Lock()
	id := c.nextID
	c.nextID++
	responseCh := make(chan rpcResponse, 1)
	c.pending[id] = responseCh
	payload, err := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": id, "method": method, "params": params})
	if err == nil {
		_, err = c.writer.Write(append(payload, '\n'))
		if err == nil {
			err = c.writer.Flush()
		}
	}
	if err != nil {
		delete(c.pending, id)
	}
	c.mu.Unlock()
	if err != nil {
		return fmt.Errorf("send %s: %w", method, err)
	}
	select {
	case response, open := <-responseCh:
		if !open {
			return fmt.Errorf("%s: codex app-server closed", method)
		}
		if response.Error != nil {
			// The JSON-RPC message is upstream text; only its numeric code is
			// kept.
			return fmt.Errorf("%s: codex app-server error %d", method, response.Error.Code)
		}
		if target == nil || len(response.Result) == 0 || string(response.Result) == "null" {
			return nil
		}
		if err := json.Unmarshal(response.Result, target); err != nil {
			return fmt.Errorf("decode %s response: %w", method, err)
		}
		return nil
	case <-ctx.Done():
		c.mu.Lock()
		delete(c.pending, id)
		c.mu.Unlock()
		return ctx.Err()
	case <-c.done:
		return fmt.Errorf("%s: codex app-server closed", method)
	}
}

func (c *appServerClient) notify(method string, params any) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	payload, err := json.Marshal(map[string]any{"jsonrpc": "2.0", "method": method, "params": params})
	if err != nil {
		return err
	}
	if _, err := c.writer.Write(append(payload, '\n')); err != nil {
		return err
	}
	return c.writer.Flush()
}

// turnResult is what one Codex turn produced.
type turnResult struct {
	content string
	// usage is the latest token accounting reported for the turn; nil when
	// none arrived before the turn completed.
	usage *TokenUsage
	// firstText is when the turn's first answer text arrived; zero when the
	// turn produced none.
	firstText time.Time
}

// waitForTurn reads the turn's notifications until it completes. A failed
// turn becomes a typed, content-free error: a usage limit is a
// *RateLimitError carrying the reset of the latest account/rateLimits/updated
// snapshot when one showed an exhausted window, anything else a turn_failed
// *UpstreamError naming the codexErrorInfo variant. The error's message and
// additionalDetails are never read.
func (c *appServerClient) waitForTurn(ctx context.Context, threadID, turnID string, onDelta func(string) error) (turnResult, error) {
	var content strings.Builder
	var result turnResult
	// lastError is the codexErrorInfo of the latest error notification that
	// ends the turn; retries counts those Codex retried (willRetry:true), and
	// retryStatus is the upstream HTTP status the latest of them named (0 when
	// none): Codex reports each retry as responseStreamDisconnected carrying
	// the failed attempt's status.
	var lastError codexErrorInfo
	retries, retryStatus := 0, 0
	var rateLimits *codexRateLimitSnapshot
	markText := func(text string) {
		if text != "" && result.firstText.IsZero() {
			result.firstText = time.Now()
		}
	}
	for {
		select {
		case event, open := <-c.events:
			if !open {
				return turnResult{}, errors.New("codex app-server closed before the turn completed")
			}
			switch event.Method {
			case "item/agentMessage/delta":
				var delta struct {
					ThreadID string `json:"threadId"`
					TurnID   string `json:"turnId"`
					Delta    string `json:"delta"`
				}
				if json.Unmarshal(event.Params, &delta) == nil && matchesTurn(threadID, turnID, delta.ThreadID, delta.TurnID) {
					markText(delta.Delta)
					content.WriteString(delta.Delta)
					if onDelta != nil && delta.Delta != "" {
						if err := onDelta(delta.Delta); err != nil {
							return turnResult{}, fmt.Errorf("stream Codex subscription delta: %w", err)
						}
					}
				}
			case "thread/tokenUsage/updated":
				// Codex reports usage after each model response in the turn.
				// Keep the latest thread total: the thread is ephemeral and
				// runs only this turn, so its total is the turn's. `last`
				// would cover just the most recent model response.
				var updated struct {
					ThreadID   string `json:"threadId"`
					TurnID     string `json:"turnId"`
					TokenUsage struct {
						Total *TokenUsage `json:"total"`
					} `json:"tokenUsage"`
				}
				if json.Unmarshal(event.Params, &updated) == nil && updated.TokenUsage.Total != nil && matchesTurn(threadID, turnID, updated.ThreadID, updated.TurnID) {
					result.usage = updated.TokenUsage.Total
				}
			case "item/completed":
				// Some app-server releases stream only completion items. Use their
				// text only when deltas did not already provide the final answer.
				if content.Len() == 0 {
					var item struct {
						ThreadID string `json:"threadId"`
						TurnID   string `json:"turnId"`
						Item     struct {
							Type string `json:"type"`
							Text string `json:"text"`
						} `json:"item"`
					}
					if json.Unmarshal(event.Params, &item) == nil && item.Item.Type == "agentMessage" && matchesTurn(threadID, turnID, item.ThreadID, item.TurnID) {
						markText(item.Item.Text)
						content.WriteString(item.Item.Text)
						if onDelta != nil && item.Item.Text != "" {
							if err := onDelta(item.Item.Text); err != nil {
								return turnResult{}, fmt.Errorf("stream completed Codex subscription item: %w", err)
							}
						}
					}
				}
			case "error":
				var notification struct {
					Error struct {
						CodexErrorInfo json.RawMessage `json:"codexErrorInfo"`
					} `json:"error"`
					WillRetry bool   `json:"willRetry"`
					ThreadID  string `json:"threadId"`
					TurnID    string `json:"turnId"`
				}
				if json.Unmarshal(event.Params, &notification) == nil && matchesTurn(threadID, turnID, notification.ThreadID, notification.TurnID) {
					if notification.WillRetry {
						retries++
						retryStatus = parseCodexErrorInfo(notification.Error.CodexErrorInfo).httpStatus
					} else {
						lastError = parseCodexErrorInfo(notification.Error.CodexErrorInfo)
					}
				}
			case "account/rateLimits/updated":
				// Account-wide, not turn-scoped: the process serves one turn.
				var updated struct {
					RateLimits *codexRateLimitSnapshot `json:"rateLimits"`
				}
				if json.Unmarshal(event.Params, &updated) == nil && updated.RateLimits != nil {
					if rateLimits == nil {
						rateLimits = &codexRateLimitSnapshot{}
					}
					rateLimits.merge(*updated.RateLimits)
				}
			case "turn/completed":
				var completed struct {
					ThreadID string `json:"threadId"`
					Turn     struct {
						ID     string `json:"id"`
						Status string `json:"status"`
						Error  *struct {
							CodexErrorInfo json.RawMessage `json:"codexErrorInfo"`
						} `json:"error"`
					} `json:"turn"`
				}
				if json.Unmarshal(event.Params, &completed) == nil && matchesTurn(threadID, turnID, completed.ThreadID, completed.Turn.ID) {
					switch completed.Turn.Status {
					case "", "completed":
						result.content = content.String()
						return result, nil
					case "failed":
						info := lastError
						if completed.Turn.Error != nil {
							if reported := parseCodexErrorInfo(completed.Turn.Error.CodexErrorInfo); reported.present {
								info = reported
							}
						}
						return turnResult{}, turnFailureError(info, retries, retryStatus, rateLimits, time.Now())
					default:
						return turnResult{}, fmt.Errorf("codex subscription turn ended with status %s", sanitizeTurnStatus(completed.Turn.Status))
					}
				}
			}
		case <-ctx.Done():
			return turnResult{}, ctx.Err()
		case <-c.done:
			return turnResult{}, errors.New("codex app-server closed before the turn completed")
		}
	}
}

func matchesTurn(expectedThread, expectedTurn, actualThread, actualTurn string) bool {
	return (actualThread == "" || actualThread == expectedThread) && (actualTurn == "" || actualTurn == expectedTurn)
}

// codexErrorVariants maps the pinned Codex 0.153.4 codexErrorInfo variants
// (app-server-protocol v2 shared.rs CodexErrorInfo) to their snake_case log
// and classification names.
var codexErrorVariants = map[string]string{
	"contextWindowExceeded":          "context_window_exceeded",
	"sessionBudgetExceeded":          "session_budget_exceeded",
	"usageLimitExceeded":             "usage_limit_exceeded",
	"rateLimitExceeded":              "rate_limit_exceeded",
	"serverOverloaded":               "server_overloaded",
	"cyberPolicy":                    "cyber_policy",
	"misalignmentPolicyViolation":    "misalignment_policy_violation",
	"httpConnectionFailed":           "http_connection_failed",
	"responseStreamConnectionFailed": "response_stream_connection_failed",
	"internalServerError":            "internal_server_error",
	"unauthorized":                   "unauthorized",
	"badRequest":                     "bad_request",
	"threadRollbackFailed":           "thread_rollback_failed",
	"sandboxError":                   "sandbox_error",
	"responseStreamDisconnected":     "response_stream_disconnected",
	"responseTooManyFailedAttempts":  "response_too_many_failed_attempts",
	"activeTurnNotSteerable":         "active_turn_not_steerable",
	"other":                          upstreamCodeOther,
}

// codexErrorInfo is a parsed codexErrorInfo: the allowlisted variant, or
// "unrecognized", and the upstream HTTP status a variant's payload named.
type codexErrorInfo struct {
	present    bool
	code       string
	httpStatus int
}

// parseCodexErrorInfo reads a unit variant (a JSON string) or a variant with
// a payload (a single-key object, whose httpStatusCode is an int or null).
// Nothing else of the payload is kept.
func parseCodexErrorInfo(raw json.RawMessage) codexErrorInfo {
	if len(raw) == 0 || string(raw) == "null" {
		return codexErrorInfo{}
	}
	info := codexErrorInfo{present: true, code: "unrecognized"}
	var unit string
	if json.Unmarshal(raw, &unit) == nil {
		if code, known := codexErrorVariants[unit]; known {
			info.code = code
		}
		return info
	}
	var tagged map[string]json.RawMessage
	if json.Unmarshal(raw, &tagged) != nil || len(tagged) != 1 {
		return info
	}
	for variant, payload := range tagged {
		code, known := codexErrorVariants[variant]
		if !known {
			return info
		}
		info.code = code
		var fields struct {
			HTTPStatusCode *int `json:"httpStatusCode"`
		}
		if json.Unmarshal(payload, &fields) == nil && fields.HTTPStatusCode != nil && *fields.HTTPStatusCode >= 100 && *fields.HTTPStatusCode <= 599 {
			info.httpStatus = *fields.HTTPStatusCode
		}
	}
	return info
}

// turnFailureError is the typed error of a failed turn. A usage limit takes
// its reset from the kept rate-limit snapshot when a window there is
// exhausted; otherwise the reset stays unknown until invoke reads it.
// retryStatus is the HTTP status of Codex's latest retried attempt, 0 when
// none was named.
func turnFailureError(info codexErrorInfo, retries, retryStatus int, snapshot *codexRateLimitSnapshot, now time.Time) error {
	if info.code == "usage_limit_exceeded" {
		limited := &RateLimitError{Kind: RateLimitUsage, upstreamCode: info.code, resetSource: "none"}
		if snapshot != nil {
			limited.limitID = snapshot.limitIDValue()
			limited.LimitReachedType = snapshot.limitReachedType()
			if limited.applyExhaustedWindow(*snapshot, now) {
				limited.resetSource = "notification"
			}
		}
		return limited
	}
	code := info.code
	if !info.present {
		code = "none"
	}
	return &UpstreamError{Kind: upstreamKindTurnFailed, Code: code, HTTPStatus: info.httpStatus, upstreamRetries: retries, retryHTTPStatus: retryStatus}
}

// sanitizeTurnStatus keeps a turn status only when it is a pinned TurnStatus
// value, so an error string never carries arbitrary upstream text.
func sanitizeTurnStatus(status string) string {
	switch status {
	case "interrupted", "failed", "inProgress":
		return status
	}
	return "unrecognized"
}

// codexRateLimitWindow is the pinned RateLimitWindow: unix-seconds resetsAt.
type codexRateLimitWindow struct {
	UsedPercent        float64  `json:"usedPercent"`
	WindowDurationMins *float64 `json:"windowDurationMins"`
	ResetsAt           *float64 `json:"resetsAt"`
}

// codexRateLimitSnapshot is the part of the pinned RateLimitSnapshot the
// broker reads. planType is deliberately absent: it is never read.
type codexRateLimitSnapshot struct {
	LimitID              *string               `json:"limitId"`
	Primary              *codexRateLimitWindow `json:"primary"`
	Secondary            *codexRateLimitWindow `json:"secondary"`
	RateLimitReachedType *string               `json:"rateLimitReachedType"`
}

// merge applies a sparse account/rateLimits/updated snapshot: its non-null
// values replace the kept ones, and an update for another bucket replaces
// the whole snapshot.
func (s *codexRateLimitSnapshot) merge(update codexRateLimitSnapshot) {
	if update.LimitID != nil && s.LimitID != nil && *update.LimitID != *s.LimitID {
		*s = update
		return
	}
	if update.LimitID != nil {
		s.LimitID = update.LimitID
	}
	if update.Primary != nil {
		s.Primary = update.Primary
	}
	if update.Secondary != nil {
		s.Secondary = update.Secondary
	}
	if update.RateLimitReachedType != nil {
		s.RateLimitReachedType = update.RateLimitReachedType
	}
}

func (s codexRateLimitSnapshot) limitIDValue() string {
	if s.LimitID == nil || !rateLimitFamilyPattern.MatchString(strings.ReplaceAll(*s.LimitID, "_", "-")) {
		return ""
	}
	return *s.LimitID
}

func (s codexRateLimitSnapshot) limitReachedType() string {
	if s.RateLimitReachedType == nil {
		return ""
	}
	return allowedLimitReachedType(*s.RateLimitReachedType)
}

// exhaustedWindow picks the windows with usedPercent >= 100 and a resetsAt
// inside (now, now+7d], the later reset when both qualify, with that
// window's length.
func (s codexRateLimitSnapshot) exhaustedWindow(now time.Time) (time.Time, int, bool) {
	var resetsAt time.Time
	minutes, found := 0, false
	for _, window := range []*codexRateLimitWindow{s.Primary, s.Secondary} {
		if window == nil || window.ResetsAt == nil || window.UsedPercent < 100 {
			continue
		}
		reset, ok := unixReset(*window.ResetsAt, now)
		if !ok || (found && !reset.After(resetsAt)) {
			continue
		}
		resetsAt, minutes, found = reset, 0, true
		if window.WindowDurationMins != nil && *window.WindowDurationMins > 0 && *window.WindowDurationMins <= maxWindowMinutes {
			minutes = int(*window.WindowDurationMins)
		}
	}
	return resetsAt, minutes, found
}

// applyExhaustedWindow sets the usage limit's reset and window from a
// snapshot, reporting whether one of its windows was exhausted.
func (e *RateLimitError) applyExhaustedWindow(snapshot codexRateLimitSnapshot, now time.Time) bool {
	resetsAt, minutes, ok := snapshot.exhaustedWindow(now)
	if !ok {
		return false
	}
	e.ResetsAt, e.WindowMinutes, e.RetryAfter = resetsAt, minutes, clampRetryAfter(resetsAt.Sub(now))
	return true
}

// Close kills the app-server's whole process group (the Node shim, the
// native codex binary, and any MCP or npm children), waits briefly for it,
// and removes its ephemeral state directory.
func (c *appServerClient) Close() error {
	var closeErr error
	c.close.Do(func() {
		_ = c.stdin.Close()
		if c.cmd.Process != nil {
			if err := killProcessTree(c.cmd); err != nil {
				closeErr = err
			}
		}
		waitDone := make(chan struct{})
		go func() {
			_ = c.cmd.Wait()
			close(waitDone)
		}()
		select {
		case <-waitDone:
		case <-time.After(2 * time.Second):
		}
		if c.ephemeralDir != "" {
			_ = os.RemoveAll(c.ephemeralDir)
		}
	})
	return closeErr
}

func renderPrompt(messages []ChatMessage) string {
	var prompt strings.Builder
	for _, message := range messages {
		role := strings.TrimSpace(message.Role)
		if role == "" {
			role = "user"
		}
		prompt.WriteString(strings.ToUpper(role))
		if name := strings.TrimSpace(message.Name); name != "" {
			prompt.WriteString(" (")
			prompt.WriteString(name)
			prompt.WriteString(")")
		}
		prompt.WriteString(":\n")
		prompt.WriteString(message.Content)
		prompt.WriteString("\n\n")
	}
	return prompt.String()
}
