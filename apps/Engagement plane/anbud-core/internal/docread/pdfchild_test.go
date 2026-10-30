package docread

import (
	"context"
	"errors"
	"os"
	"strings"
	"testing"
	"time"
)

func child(mode string, mutate ...func(*ChildPDF)) *ChildPDF {
	c := &ChildPDF{
		Exe:       os.Args[0],
		Env:       []string{childModeEnv + "=" + mode},
		Timeout:   10 * time.Second,
		MaxOutput: 1 << 20,
	}
	for _, f := range mutate {
		f(c)
	}
	c.once.Do(c.setup)
	return c
}

func TestChildPDFReadsAPDFInAnotherProcess(t *testing.T) {
	t.Parallel()

	want, err := ExtractPDF(fixturePDF(t), 100)
	if err != nil {
		t.Fatalf("ExtractPDF: %v", err)
	}
	got, err := child("real").Run(context.Background(), fixturePDF(t), 100)
	if err != nil {
		t.Fatalf("Run: %v", err)
	}
	if got.TotalPages != want.TotalPages || len(got.Pages) != len(want.Pages) || got.Pages[0] != want.Pages[0] || got.Pages[1] != want.Pages[1] {
		t.Errorf("the child read %+v, the same process %+v", got, want)
	}
}

func TestChildPDFPassesThePageLimit(t *testing.T) {
	t.Parallel()

	got, err := child("real").Run(context.Background(), miniPDF("en", "to", "tre", "fire"), 2)
	if err != nil {
		t.Fatalf("Run: %v", err)
	}
	if len(got.Pages) != 2 || got.TotalPages != 4 || !got.Truncated {
		t.Errorf("result = %+v, want 2 of 4 pages and truncated", got)
	}
}

func TestChildPDFAnswersMalformedForADamagedFile(t *testing.T) {
	t.Parallel()

	if _, err := child("real").Run(context.Background(), []byte("%PDF-1.4\nnonsense"), 100); !errors.Is(err, ErrMalformed) {
		t.Errorf("err = %v, want ErrMalformed", err)
	}
}

func TestChildPDFIsKilledWhenItTakesTooLong(t *testing.T) {
	t.Parallel()

	start := time.Now()
	_, err := child("sleep", func(c *ChildPDF) { c.Timeout = 300 * time.Millisecond }).Run(context.Background(), fixturePDF(t), 100)
	if !errors.Is(err, ErrTimeout) {
		t.Errorf("err = %v, want ErrTimeout", err)
	}
	if took := time.Since(start); took > 8*time.Second {
		t.Errorf("took %v: the child was not killed at its deadline", took)
	}
}

func TestChildPDFIsKilledWhenItWritesTooMuch(t *testing.T) {
	t.Parallel()

	start := time.Now()
	_, err := child("flood", func(c *ChildPDF) { c.MaxOutput = 1 << 16 }).Run(context.Background(), fixturePDF(t), 100)
	if !errors.Is(err, ErrFailed) {
		t.Errorf("err = %v, want ErrFailed", err)
	}
	if took := time.Since(start); took > 8*time.Second {
		t.Errorf("took %v: a child that floods must be stopped at once", took)
	}
}

func TestChildPDFMapsWhatTheChildSays(t *testing.T) {
	t.Parallel()

	for name, tc := range map[string]struct {
		mode string
		want error
	}{
		"it crashes":                {"crash", ErrFailed},
		"it writes no json":         {"garbage", ErrFailed},
		"it says encrypted":         {"error-code", ErrEncrypted},
		"it says something unknown": {"unknown-error", ErrFailed},
	} {
		if _, err := child(tc.mode).Run(context.Background(), fixturePDF(t), 100); !errors.Is(err, tc.want) {
			t.Errorf("%s: err = %v, want %v", name, err, tc.want)
		}
	}
}

// The child parses a file a stranger sent, and so runs with nothing of the service's: not its
// database password, not its API key. The test plays a child that reports what it can see.
func TestChildPDFSeesNothingOfTheParentsEnvironment(t *testing.T) {
	t.Setenv("ANBUD_SECRET_PROBE", "hunter2-not-for-children")

	got, err := child("leak").Run(context.Background(), fixturePDF(t), 100)
	if err != nil {
		t.Fatalf("Run: %v", err)
	}
	if len(got.Pages) != 1 || got.Pages[0] != "" {
		t.Errorf("the child saw %q of its parent's environment", got.Pages)
	}
}

func TestChildPDFRunsNoMoreChildrenThanItMay(t *testing.T) {
	t.Parallel()

	c := child("sleep", func(c *ChildPDF) { c.Timeout = 2 * time.Second; c.MaxConcurrent = 1 })
	started := make(chan struct{})
	done := make(chan error, 1)
	go func() {
		close(started)
		_, err := c.Run(context.Background(), fixturePDF(t), 100)
		done <- err
	}()
	<-started
	time.Sleep(200 * time.Millisecond) // the first child holds the only slot

	ctx, cancel := context.WithTimeout(context.Background(), 200*time.Millisecond)
	defer cancel()
	start := time.Now()
	if _, err := c.Run(ctx, fixturePDF(t), 100); !errors.Is(err, ErrTimeout) {
		t.Errorf("a second run while the slot is taken: err = %v, want ErrTimeout", err)
	}
	if time.Since(start) > time.Second {
		t.Errorf("the second run waited %v for a slot it was never going to get", time.Since(start))
	}
	if err := <-done; !errors.Is(err, ErrTimeout) {
		t.Errorf("the first run: err = %v, want ErrTimeout from its own deadline", err)
	}
}

func TestChildPDFFailsCleanlyWhenTheProgramCannotStart(t *testing.T) {
	t.Parallel()

	c := &ChildPDF{Exe: "/nonexistent/anbud-pdf-child", Timeout: time.Second, MaxOutput: 1 << 10}
	c.once.Do(c.setup)
	if _, err := c.Run(context.Background(), fixturePDF(t), 100); !errors.Is(err, ErrFailed) {
		t.Errorf("err = %v, want ErrFailed", err)
	}
}

func TestRunPDFChildRefusesInputOverItsLimit(t *testing.T) {
	t.Parallel()

	var out strings.Builder
	code := RunPDFChild([]string{"--max-input=100"}, strings.NewReader(strings.Repeat("x", 500)), &out)
	if code != 0 || !strings.Contains(out.String(), `"error":"too_large"`) {
		t.Errorf("exit %d output %q, want a too_large answer", code, out.String())
	}
}
