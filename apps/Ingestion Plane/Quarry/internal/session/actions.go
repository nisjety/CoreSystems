package session

import (
	"context"
	"fmt"
	"io"
	"strings"
	"time"

	"github.com/go-rod/rod/lib/input"
	"github.com/go-rod/rod/lib/proto"
)

// ActionType enumerates supported browser actions.
type ActionType string

const (
	ActionClick      ActionType = "click"
	ActionType_      ActionType = "type"
	ActionPress      ActionType = "press"
	ActionScroll     ActionType = "scroll"
	ActionWait       ActionType = "wait"
	ActionNavigate   ActionType = "navigate"
	ActionEvalJS     ActionType = "execute_javascript"
	ActionPDF        ActionType = "generate_pdf"
	ActionScreenshot ActionType = "screenshot"
	ActionExtract    ActionType = "extract"
	ActionDone       ActionType = "done"
)

// Action describes a single browser action to execute.
type Action struct {
	Type     ActionType `json:"type"`
	Selector string     `json:"selector,omitempty"`
	Value    string     `json:"value,omitempty"`
	WaitMs   int        `json:"wait_ms,omitempty"`
	FullPage bool       `json:"full_page,omitempty"` // for screenshot
}

// ActionResult holds the result of a browser action.
type ActionResult struct {
	Type       ActionType `json:"type"`
	Success    bool       `json:"success"`
	Error      string     `json:"error,omitempty"`
	Data       string     `json:"data,omitempty"`       // text output (HTML, JS result, etc.)
	BinaryB64  string     `json:"binary_b64,omitempty"` // base64-encoded binary (screenshot, PDF)
	DurationMs int64      `json:"duration_ms"`
}

// Execute runs a single action on the session's page. Thread-safe.
func (s *BrowserSession) Execute(ctx context.Context, action Action) ActionResult {
	s.mu.Lock()
	defer s.mu.Unlock()

	if s.closed || s.page == nil {
		return ActionResult{Type: action.Type, Success: false, Error: "session closed"}
	}

	s.StepCount++
	s.LastUsed = time.Now()
	start := time.Now()

	result := s.executeUnsafe(ctx, action)
	result.DurationMs = time.Since(start).Milliseconds()
	return result
}

// ExecuteBatch runs multiple actions sequentially. Stops on first error unless continueOnError is set.
func (s *BrowserSession) ExecuteBatch(ctx context.Context, actions []Action, continueOnError bool) []ActionResult {
	results := make([]ActionResult, 0, len(actions))
	for _, a := range actions {
		if err := ctx.Err(); err != nil {
			results = append(results, ActionResult{Type: a.Type, Success: false, Error: err.Error()})
			break
		}
		r := s.Execute(ctx, a)
		results = append(results, r)
		if !r.Success && !continueOnError {
			break
		}
		// Respect wait_ms between actions
		if a.WaitMs > 0 {
			select {
			case <-ctx.Done():
				return results
			case <-time.After(time.Duration(a.WaitMs) * time.Millisecond):
			}
		}
	}
	return results
}

// HTML returns the current page HTML.
func (s *BrowserSession) HTML(ctx context.Context) (string, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	if s.closed || s.page == nil {
		return "", fmt.Errorf("session closed")
	}
	s.LastUsed = time.Now()
	return s.page.HTML()
}

// Screenshot captures the current page as a PNG and returns it base64-encoded.
func (s *BrowserSession) Screenshot(ctx context.Context, fullPage bool) (string, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	if s.closed || s.page == nil {
		return "", fmt.Errorf("session closed")
	}
	if err := ctx.Err(); err != nil {
		return "", err
	}
	s.LastUsed = time.Now()
	data, err := s.page.Screenshot(fullPage, &proto.PageCaptureScreenshot{
		Format:                proto.PageCaptureScreenshotFormatPng,
		FromSurface:           true,
		CaptureBeyondViewport: fullPage,
	})
	if err != nil {
		return "", err
	}
	return encodeBase64(data), nil
}

// CurrentURL returns the current page URL.
func (s *BrowserSession) CurrentURL() string {
	s.mu.Lock()
	defer s.mu.Unlock()

	if s.closed || s.page == nil {
		return ""
	}
	info, err := s.page.Info()
	if err != nil {
		return s.URL
	}
	return info.URL
}

func (s *BrowserSession) executeUnsafe(ctx context.Context, action Action) ActionResult {
	if err := ctx.Err(); err != nil {
		return ActionResult{Type: action.Type, Success: false, Error: err.Error()}
	}

	switch action.Type {
	case ActionClick:
		return s.doClick(action)
	case ActionType_:
		return s.doType(action)
	case ActionPress:
		return s.doPress(action)
	case ActionScroll:
		return s.doScroll(action)
	case ActionWait:
		return s.doWait(ctx, action)
	case ActionNavigate:
		return s.doNavigate(action)
	case ActionEvalJS:
		return s.doEvalJS(action)
	case ActionPDF:
		return s.doPDF()
	case ActionScreenshot:
		return s.doScreenshot(action)
	case ActionExtract:
		return s.doExtractHTML()
	case ActionDone:
		return ActionResult{Type: ActionDone, Success: true}
	default:
		return ActionResult{Type: action.Type, Success: false, Error: fmt.Sprintf("unsupported action: %s", action.Type)}
	}
}

func (s *BrowserSession) doClick(a Action) ActionResult {
	if a.Selector == "" {
		return ActionResult{Type: a.Type, Success: false, Error: "selector required"}
	}
	el, err := s.page.Element(a.Selector)
	if err != nil {
		return ActionResult{Type: a.Type, Success: false, Error: fmt.Sprintf("element not found: %v", err)}
	}
	if err := el.Click(proto.InputMouseButtonLeft, 1); err != nil {
		return ActionResult{Type: a.Type, Success: false, Error: fmt.Sprintf("click failed: %v", err)}
	}
	return ActionResult{Type: a.Type, Success: true}
}

func (s *BrowserSession) doType(a Action) ActionResult {
	if a.Selector == "" {
		return ActionResult{Type: a.Type, Success: false, Error: "selector required"}
	}
	el, err := s.page.Element(a.Selector)
	if err != nil {
		return ActionResult{Type: a.Type, Success: false, Error: fmt.Sprintf("element not found: %v", err)}
	}
	// Clear existing content first, then type.
	if err := el.SelectAllText(); err == nil {
		_ = el.Input("")
	}
	if err := el.Input(a.Value); err != nil {
		return ActionResult{Type: a.Type, Success: false, Error: fmt.Sprintf("type failed: %v", err)}
	}
	return ActionResult{Type: a.Type, Success: true}
}

func (s *BrowserSession) doPress(a Action) ActionResult {
	code, ok := mapKey(a.Value)
	if !ok {
		return ActionResult{Type: a.Type, Success: false, Error: fmt.Sprintf("unsupported key: %s", a.Value)}
	}
	if err := s.page.Keyboard.Press(code); err != nil {
		return ActionResult{Type: a.Type, Success: false, Error: fmt.Sprintf("press failed: %v", err)}
	}
	return ActionResult{Type: a.Type, Success: true}
}

func (s *BrowserSession) doScroll(a Action) ActionResult {
	dir := strings.ToLower(strings.TrimSpace(a.Value))
	var script string
	switch dir {
	case "up":
		script = `window.scrollBy(0, -600);`
	case "down", "":
		script = `window.scrollBy(0, 600);`
	case "left":
		script = `window.scrollBy(-600, 0);`
	case "right":
		script = `window.scrollBy(600, 0);`
	case "top":
		script = `window.scrollTo(0, 0);`
	case "bottom":
		script = `window.scrollTo(0, document.body.scrollHeight);`
	default:
		script = `window.scrollBy(0, 600);`
	}
	if _, err := s.page.Eval(script); err != nil {
		return ActionResult{Type: a.Type, Success: false, Error: fmt.Sprintf("scroll failed: %v", err)}
	}
	return ActionResult{Type: a.Type, Success: true}
}

func (s *BrowserSession) doWait(ctx context.Context, a Action) ActionResult {
	ms := a.WaitMs
	if ms <= 0 {
		ms = 1000
	}
	if ms > 30000 {
		ms = 30000 // cap at 30s
	}
	t := time.NewTimer(time.Duration(ms) * time.Millisecond)
	defer t.Stop()
	select {
	case <-ctx.Done():
		return ActionResult{Type: a.Type, Success: false, Error: ctx.Err().Error()}
	case <-t.C:
		return ActionResult{Type: a.Type, Success: true}
	}
}

func (s *BrowserSession) doNavigate(a Action) ActionResult {
	if a.Value == "" {
		return ActionResult{Type: a.Type, Success: false, Error: "url required in value"}
	}
	if err := s.page.Navigate(a.Value); err != nil {
		return ActionResult{Type: a.Type, Success: false, Error: fmt.Sprintf("navigate failed: %v", err)}
	}
	if err := s.page.WaitLoad(); err != nil {
		return ActionResult{Type: a.Type, Success: false, Error: fmt.Sprintf("wait load failed: %v", err)}
	}
	return ActionResult{Type: a.Type, Success: true}
}

func (s *BrowserSession) doEvalJS(a Action) ActionResult {
	if a.Value == "" {
		return ActionResult{Type: a.Type, Success: false, Error: "script required in value"}
	}
	wrapped := fmt.Sprintf("() => { return %s }", a.Value)
	obj, err := s.page.Eval(wrapped)
	if err != nil {
		return ActionResult{Type: a.Type, Success: false, Error: fmt.Sprintf("eval failed: %v", err)}
	}
	var val interface{}
	if err := obj.Value.Unmarshal(&val); err != nil {
		return ActionResult{Type: a.Type, Success: true, Data: obj.Value.Str()}
	}
	return ActionResult{Type: a.Type, Success: true, Data: fmt.Sprintf("%v", val)}
}

func (s *BrowserSession) doPDF() ActionResult {
	reader, err := s.page.PDF(&proto.PagePrintToPDF{PrintBackground: true})
	if err != nil {
		return ActionResult{Type: ActionPDF, Success: false, Error: fmt.Sprintf("pdf failed: %v", err)}
	}
	data, err := io.ReadAll(reader)
	if err != nil {
		return ActionResult{Type: ActionPDF, Success: false, Error: fmt.Sprintf("read pdf: %v", err)}
	}
	return ActionResult{
		Type:      ActionPDF,
		Success:   true,
		BinaryB64: encodeBase64(data),
	}
}

func (s *BrowserSession) doScreenshot(a Action) ActionResult {
	data, err := s.page.Screenshot(a.FullPage, nil)
	if err != nil {
		return ActionResult{Type: ActionScreenshot, Success: false, Error: fmt.Sprintf("screenshot failed: %v", err)}
	}
	return ActionResult{
		Type:      ActionScreenshot,
		Success:   true,
		BinaryB64: encodeBase64(data),
	}
}

func (s *BrowserSession) doExtractHTML() ActionResult {
	html, err := s.page.HTML()
	if err != nil {
		return ActionResult{Type: ActionExtract, Success: false, Error: fmt.Sprintf("html extraction failed: %v", err)}
	}
	return ActionResult{Type: ActionExtract, Success: true, Data: html}
}

// mapKey maps string key names to Rod input.Key codes.
func mapKey(name string) (input.Key, bool) {
	switch strings.ToLower(strings.TrimSpace(name)) {
	case "enter", "return":
		return input.Enter, true
	case "tab":
		return input.Tab, true
	case "escape", "esc":
		return input.Escape, true
	case "backspace":
		return input.Backspace, true
	case "delete":
		return input.Delete, true
	case "arrowup", "up":
		return input.ArrowUp, true
	case "arrowdown", "down":
		return input.ArrowDown, true
	case "arrowleft", "left":
		return input.ArrowLeft, true
	case "arrowright", "right":
		return input.ArrowRight, true
	case "space":
		return input.Space, true
	case "home":
		return input.Home, true
	case "end":
		return input.End, true
	case "pageup":
		return input.PageUp, true
	case "pagedown":
		return input.PageDown, true
	default:
		return 0, false
	}
}
