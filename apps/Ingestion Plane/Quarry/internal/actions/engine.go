package actions

import (
	"context"
	"encoding/base64"
	"fmt"
	"strings"
	"time"

	"github.com/triodelab/quarry/internal/driver"
)

type Engine struct{}

func NewEngine() *Engine { return &Engine{} }

func (e *Engine) Execute(ctx context.Context, drv driver.PageDriver, steps []ActionStep) ([]ActionResult, error) {
	if len(steps) == 0 {
		return []ActionResult{}, nil
	}
	results := make([]ActionResult, 0, len(steps))

	for _, step := range steps {
		select {
		case <-ctx.Done():
			return results, ctx.Err()
		default:
		}

		started := time.Now()
		t := strings.ToLower(strings.TrimSpace(step.Type))

		// executeJavascript and generatePDF need to capture their return values.
		switch t {
		case "executejavascript", "executescript", "evaljs":
			if step.Script == "" {
				err := fmt.Errorf("script is required for executeJavascript")
				results = append(results, ActionResult{Type: step.Type, Success: false, DurationMs: time.Since(started).Milliseconds(), Error: err.Error()})
				return results, fmt.Errorf("action %s failed: %w", step.Type, err)
			}
			val, err := drv.EvalJS(ctx, step.Script)
			result := ActionResult{Type: step.Type, DurationMs: time.Since(started).Milliseconds()}
			if err != nil {
				result.Error = err.Error()
				results = append(results, result)
				return results, fmt.Errorf("action %s failed: %w", step.Type, err)
			}
			result.Success = true
			result.Output = val
			results = append(results, result)

		case "generatepdf":
			pdfBytes, err := drv.GeneratePDF(ctx)
			result := ActionResult{Type: step.Type, DurationMs: time.Since(started).Milliseconds()}
			if err != nil {
				result.Error = err.Error()
				results = append(results, result)
				return results, fmt.Errorf("action %s failed: %w", step.Type, err)
			}
			result.Success = true
			// Base64-encode the PDF so it survives JSON serialisation.
			result.Output = base64.StdEncoding.EncodeToString(pdfBytes)
			results = append(results, result)

		default:
			err := executeWithRetry(ctx, drv, step)
			if err == nil && step.AfterShot {
				_, err = drv.Screenshot(ctx, step.FullPage)
			}
			result := ActionResult{
				Type:       step.Type,
				Success:    err == nil,
				DurationMs: time.Since(started).Milliseconds(),
			}
			if err != nil {
				result.Error = err.Error()
				results = append(results, result)
				return results, fmt.Errorf("action %s failed: %w", step.Type, err)
			}
			results = append(results, result)
		}
	}
	return results, nil
}

func executeWithRetry(ctx context.Context, drv driver.PageDriver, step ActionStep) error {
	attempts := step.Retry + 1
	if attempts < 1 {
		attempts = 1
	}
	var lastErr error
	for i := 0; i < attempts; i++ {
		if err := executeStep(ctx, drv, step); err != nil {
			lastErr = err
			continue
		}
		return nil
	}
	if lastErr != nil {
		return lastErr
	}
	return fmt.Errorf("action failed")
}

func executeStep(ctx context.Context, drv driver.PageDriver, step ActionStep) error {
	switch strings.ToLower(strings.TrimSpace(step.Type)) {
	case "click":
		if step.Selector == "" {
			return fmt.Errorf("selector is required for click")
		}
		return drv.Click(ctx, step.Selector)
	case "write", "type":
		if step.Selector == "" {
			return fmt.Errorf("selector is required for type")
		}
		return drv.Type(ctx, step.Selector, step.Text)
	case "press":
		if step.Key == "" {
			return fmt.Errorf("key is required for press")
		}
		return drv.Press(ctx, step.Key)
	case "wait":
		return drv.Wait(ctx, step.Milliseconds)
	case "scroll":
		direction := step.Direction
		if strings.TrimSpace(direction) == "" {
			direction = "down"
		}
		return drv.Scroll(ctx, direction)
	case "screenshot":
		_, err := drv.Screenshot(ctx, step.FullPage)
		return err
	default:
		return fmt.Errorf("unsupported action type: %s", step.Type)
	}
}
