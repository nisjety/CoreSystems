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
	"strings"
	"sync"
	"time"
)

// ProcessRunner speaks the documented JSON-RPC-over-stdio Codex app-server
// protocol. It invokes the configured executable directly (never through a
// shell), so a configured command cannot become shell injection.
type ProcessRunner struct {
	command string
}

func NewProcessRunner(command string) *ProcessRunner {
	return &ProcessRunner{command: strings.TrimSpace(command)}
}

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
	ready, err := persistedAuthReady(codeHome)
	if err != nil {
		return InvokeResponse{}, err
	}
	if !ready {
		return InvokeResponse{}, ErrReauthenticationRequired
	}
	client, err := r.start(ctx, codeHome)
	if err != nil {
		return InvokeResponse{}, err
	}
	defer client.Close()

	// account/read makes a disconnected home fail before a prompt is accepted.
	var account json.RawMessage
	if err := client.call(ctx, "account/read", nil, &account); err != nil {
		return InvokeResponse{}, fmt.Errorf("read ChatGPT subscription account: %w", err)
	}

	workspace := filepath.Join(codeHome, "workspace")
	if err := os.MkdirAll(workspace, 0o700); err != nil {
		return InvokeResponse{}, fmt.Errorf("create isolated codex workspace: %w", err)
	}
	var thread struct {
		Thread struct {
			ID string `json:"id"`
		} `json:"thread"`
	}
	if err := client.call(ctx, "thread/start", map[string]any{
		"cwd":            workspace,
		"model":          request.Model,
		"approvalPolicy": "never",
		"sandbox":        "readOnly",
		"serviceName":    "coresystem_integration_core",
	}, &thread); err != nil {
		return InvokeResponse{}, fmt.Errorf("start Codex subscription thread: %w", err)
	}
	if strings.TrimSpace(thread.Thread.ID) == "" {
		return InvokeResponse{}, errors.New("codex app-server did not return a thread id")
	}
	var turn struct {
		Turn struct {
			ID string `json:"id"`
		} `json:"turn"`
	}
	if err := client.call(ctx, "turn/start", map[string]any{
		"threadId": thread.Thread.ID,
		"input": []map[string]string{{
			"type": "text",
			"text": renderPrompt(request.Messages),
		}},
	}, &turn); err != nil {
		return InvokeResponse{}, fmt.Errorf("start Codex subscription turn: %w", err)
	}
	content, err := client.waitForTurn(ctx, thread.Thread.ID, turn.Turn.ID)
	if err != nil {
		return InvokeResponse{}, err
	}
	if strings.TrimSpace(content) == "" {
		return InvokeResponse{}, errors.New("codex subscription turn completed without text")
	}
	return InvokeResponse{RequestID: request.RequestID, Content: content, ModelUsed: request.Model}, nil
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
	if err := client.call(ctx, "account/logout", nil, &ignored); err != nil {
		return fmt.Errorf("logout Codex subscription: %w", err)
	}
	return nil
}

func (r *ProcessRunner) start(ctx context.Context, codeHome string) (*appServerClient, error) {
	if strings.TrimSpace(r.command) == "" {
		return nil, errors.New("CODEX_APP_SERVER_COMMAND is required")
	}
	// A device-code login intentionally outlives its initiating HTTP request.
	// Invocation callers still close this process when their timeout elapses.
	// Do not bind the process lifetime to the supplied context here.
	cmd := exec.Command(r.command, codexAppServerArgs()...) // #nosec G204 -- command is trusted deployment config, not user input.
	cmd.Env = codexEnvironment(codeHome)
	stdin, err := cmd.StdinPipe()
	if err != nil {
		return nil, fmt.Errorf("create Codex app-server stdin: %w", err)
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return nil, fmt.Errorf("create Codex app-server stdout: %w", err)
	}
	// Stderr may contain provider-side diagnostic text. Do not merge it into the
	// JSON-RPC channel or application logs, which could accidentally retain a
	// prompt or account-related diagnostic.
	cmd.Stderr = io.Discard
	if err := cmd.Start(); err != nil {
		return nil, fmt.Errorf("start codex app-server: %w", err)
	}
	client := newAppServerClient(cmd, stdin, stdout)
	initCtx, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	var initialized json.RawMessage
	if err := client.call(initCtx, "initialize", map[string]any{
		"clientInfo": map[string]string{"name": "coresystem_integration_core", "title": "CoreSystem Integration Core", "version": "1"},
	}, &initialized); err != nil {
		_ = client.Close()
		return nil, fmt.Errorf("initialize codex app-server: %w", err)
	}
	if err := client.notify("initialized", map[string]any{}); err != nil {
		_ = client.Close()
		return nil, fmt.Errorf("notify codex app-server initialized: %w", err)
	}
	return client, nil
}

func codexAppServerArgs() []string {
	// The broker runs in a headless container backed by a persistent CODEX_HOME.
	// Force the documented file store so device login cannot silently choose an
	// unavailable desktop keyring and leave the database ahead of durable auth.
	return []string{"app-server", "-c", `cli_auth_credentials_store="file"`}
}

func codexEnvironment(codeHome string) []string {
	environment := make([]string, 0, len(os.Environ())+1)
	for _, entry := range os.Environ() {
		if strings.HasPrefix(strings.ToUpper(entry), "CODEX_HOME=") {
			continue
		}
		environment = append(environment, entry)
	}
	return append(environment, "CODEX_HOME="+codeHome)
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
			return fmt.Errorf("%s: %s", method, response.Error.Message)
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

func (c *appServerClient) waitForTurn(ctx context.Context, threadID, turnID string) (string, error) {
	var content strings.Builder
	for {
		select {
		case event, open := <-c.events:
			if !open {
				return "", errors.New("codex app-server closed before the turn completed")
			}
			switch event.Method {
			case "item/agentMessage/delta":
				var delta struct {
					ThreadID string `json:"threadId"`
					TurnID   string `json:"turnId"`
					Delta    string `json:"delta"`
				}
				if json.Unmarshal(event.Params, &delta) == nil && matchesTurn(threadID, turnID, delta.ThreadID, delta.TurnID) {
					content.WriteString(delta.Delta)
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
						content.WriteString(item.Item.Text)
					}
				}
			case "turn/completed":
				var completed struct {
					ThreadID string `json:"threadId"`
					Turn     struct {
						ID     string `json:"id"`
						Status string `json:"status"`
					} `json:"turn"`
				}
				if json.Unmarshal(event.Params, &completed) == nil && matchesTurn(threadID, turnID, completed.ThreadID, completed.Turn.ID) {
					if completed.Turn.Status != "" && completed.Turn.Status != "completed" {
						return "", fmt.Errorf("codex subscription turn ended with status %s", completed.Turn.Status)
					}
					return content.String(), nil
				}
			case "turn/failed":
				return "", errors.New("codex subscription turn failed")
			}
		case <-ctx.Done():
			return "", ctx.Err()
		case <-c.done:
			return "", errors.New("codex app-server closed before the turn completed")
		}
	}
}

func matchesTurn(expectedThread, expectedTurn, actualThread, actualTurn string) bool {
	return (actualThread == "" || actualThread == expectedThread) && (actualTurn == "" || actualTurn == expectedTurn)
}

func (c *appServerClient) Close() error {
	var closeErr error
	c.close.Do(func() {
		_ = c.stdin.Close()
		if c.cmd.Process != nil {
			if err := c.cmd.Process.Kill(); err != nil && !errors.Is(err, os.ErrProcessDone) {
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
	})
	return closeErr
}

func renderPrompt(messages []ChatMessage) string {
	var prompt strings.Builder
	prompt.WriteString("You are the text-only inference adapter for CoreSystem. Answer the user's request directly. Do not use tools, access files, run commands, or modify anything.\n\n")
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
