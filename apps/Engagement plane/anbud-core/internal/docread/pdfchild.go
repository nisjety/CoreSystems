package docread

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"runtime"
	"runtime/debug"
	"strconv"
	"strings"
	"sync"
	"time"
)

// ChildArg is the first argument that makes the service binary act as the PDF reader, and nothing
// else, instead of starting the service.
const ChildArg = "--pdf-extract-child"

const (
	defaultChildTimeout = 30 * time.Second
	defaultChildOutput  = 32 << 20
	defaultChildInput   = 64 << 20
	defaultChildSlots   = 2
	// defaultChildMemory is the heap at which the child ends itself. A soft limit, three quarters of it, only
	// makes the collector work harder: a file that makes the reader allocate for good needs something that
	// stops it.
	defaultChildMemory = 1 << 30
	// minChildMemory is the least the child takes as its limit: the runtime and the reader need some, and a
	// limit under it would end every child at its start.
	minChildMemory        = 16 << 20
	childWatchdogEvery    = 20 * time.Millisecond
	childOutOfMemoryExit  = 3
	childWaitDelay        = 2 * time.Second
	childReplyTooLarge    = "too_large"
	childReplyMalformed   = "malformed"
	childReplyEncrypted   = "encrypted"
	maxChildPagesArgument = 100_000
)

// ChildPDF reads PDFs in a child process: the same binary, started with ChildArg. The child holds
// nothing of the service: it is given an environment that is empty unless Env says otherwise, so the
// database password and the API key are not there to be read. It has a deadline and is killed at it,
// it may write only so much, and no more than MaxConcurrent run at once. A file that makes the
// reader loop, or allocate without end, costs one process.
type ChildPDF struct {
	// Exe is the program to run, and Args the arguments before the protocol's own. The service
	// runs its own binary with ChildArg.
	Exe  string
	Args []string
	// Env is the child's whole environment. Nil means none.
	Env           []string
	Timeout       time.Duration // 30 seconds when zero
	MaxOutput     int64         // what the child may write, 32 MiB when zero
	MaxInput      int64         // the largest PDF the child takes, 64 MiB when zero
	MaxConcurrent int           // 2 when zero
	// MaxMemory is the heap at which the child ends itself, which the parent reads as a crash: 1 GiB when
	// zero. The child's soft limit is three quarters of it.
	MaxMemory uint64

	sem  chan struct{}
	once sync.Once
}

// NewChildPDF returns a ChildPDF with its limits set. Exe and Args are the program and its leading
// arguments.
func NewChildPDF(exe string, args []string) *ChildPDF {
	c := &ChildPDF{Exe: exe, Args: args}
	c.once.Do(c.setup)
	return c
}

// setup fills in the defaults. Run does it once, before its first use, so a ChildPDF built by hand
// and shared by several goroutines is set up by one of them.
func (c *ChildPDF) setup() {
	if c.Timeout <= 0 {
		c.Timeout = defaultChildTimeout
	}
	if c.MaxOutput <= 0 {
		c.MaxOutput = defaultChildOutput
	}
	if c.MaxInput <= 0 {
		c.MaxInput = defaultChildInput
	}
	if c.MaxConcurrent <= 0 {
		c.MaxConcurrent = defaultChildSlots
	}
	if c.MaxMemory == 0 {
		c.MaxMemory = defaultChildMemory
	}
	if c.sem == nil {
		c.sem = make(chan struct{}, c.MaxConcurrent)
	}
	if c.Env == nil {
		c.Env = []string{} // an empty environment, not the parent's
	}
}

// childReply is what the child writes: the pages, or the reason it has none.
type childReply struct {
	Pages     []string `json:"pages,omitempty"`
	Total     int      `json:"total,omitempty"`
	Truncated bool     `json:"truncated,omitempty"`
	Error     string   `json:"error,omitempty"`
}

// Run reads the PDF in a child process. A caller that waits longer than ctx allows for a free slot
// gets ErrTimeout, and so does a child that outlives its deadline.
func (c *ChildPDF) Run(ctx context.Context, data []byte, maxPages int) (PDFResult, error) {
	c.once.Do(c.setup)
	select {
	case c.sem <- struct{}{}:
		defer func() { <-c.sem }()
	case <-ctx.Done():
		return PDFResult{}, ctxError(ctx, "no reader was free")
	}

	cctx, cancel := context.WithTimeout(ctx, c.Timeout)
	defer cancel()

	args := append(append([]string{}, c.Args...), "--max-pages="+strconv.Itoa(maxPages), "--max-input="+strconv.FormatInt(c.MaxInput, 10),
		"--max-memory="+strconv.FormatUint(c.MaxMemory, 10))
	cmd := exec.CommandContext(cctx, c.Exe, args...)
	cmd.Env = c.Env
	cmd.Dir = os.TempDir()
	cmd.Stdin = bytes.NewReader(data)
	out := &cappedBuffer{max: c.MaxOutput, onFull: cancel}
	cmd.Stdout = out
	cmd.WaitDelay = childWaitDelay

	runErr := cmd.Run()
	switch {
	case out.overflowed:
		return PDFResult{}, fmt.Errorf("%w: the reader wrote more than it may", ErrFailed)
	case ctx.Err() != nil:
		return PDFResult{}, ctxError(ctx, "the caller gave up")
	case cctx.Err() != nil:
		return PDFResult{}, fmt.Errorf("%w: the reader was killed at its deadline", ErrTimeout)
	}

	var reply childReply
	if err := json.Unmarshal(out.buf.Bytes(), &reply); err != nil {
		if runErr != nil {
			return PDFResult{}, fmt.Errorf("%w: the reader could not run or crashed", ErrFailed)
		}
		return PDFResult{}, fmt.Errorf("%w: the reader answered nonsense", ErrFailed)
	}
	switch reply.Error {
	case "":
	case childReplyMalformed:
		return PDFResult{}, malformed("the reader could not read the file")
	case childReplyEncrypted:
		return PDFResult{}, fmt.Errorf("%w: the file asks for a password", ErrEncrypted)
	case childReplyTooLarge:
		return PDFResult{}, ErrTooLarge
	default:
		return PDFResult{}, fmt.Errorf("%w: the reader gave an answer nobody planned for", ErrFailed)
	}
	return PDFResult{Pages: reply.Pages, TotalPages: reply.Total, Truncated: reply.Truncated}, nil
}

// ctxError maps the end of a context to this package's errors: a deadline is a timeout, and a cancel
// is the caller's own and stays one.
func ctxError(ctx context.Context, what string) error {
	if errors.Is(ctx.Err(), context.DeadlineExceeded) {
		return fmt.Errorf("%w: %s", ErrTimeout, what)
	}
	return ctx.Err()
}

// cappedBuffer collects what a child writes, up to a limit. At the limit it stops the child.
type cappedBuffer struct {
	buf        bytes.Buffer
	max        int64
	onFull     func()
	overflowed bool
}

var errOutputFull = errors.New("docread: the reader wrote more than it may")

func (b *cappedBuffer) Write(p []byte) (int, error) {
	if int64(b.buf.Len()+len(p)) > b.max {
		b.overflowed = true
		b.onFull()
		return 0, errOutputFull
	}
	return b.buf.Write(p)
}

// ChildMain is what the service binary runs when its first argument is ChildArg: it reads a PDF from
// standard input and writes the answer to standard output, and returns the exit code. It limits its own
// memory as the parent asks (--max-memory, 1 GiB when the parent does not say): a soft limit of three
// quarters of it, so that a file that makes the reader allocate meets the garbage collector early, and a
// watchdog at the limit itself, which ends the process.
func ChildMain() int {
	limitMemory(memoryArgument(os.Args[2:]))
	return RunPDFChild(os.Args[2:], os.Stdin, os.Stdout)
}

// limitMemory limits this process to hard bytes of heap: a soft limit of three quarters of it, so that a file
// that makes the reader allocate meets the garbage collector early, and a watchdog at the limit itself. It
// also asks the kernel to choose this process first when the host is out of memory.
func limitMemory(hard uint64) {
	debug.SetMemoryLimit(int64(hard / 4 * 3))
	preferToBeKilled()
	startMemoryWatchdog(hard, childWatchdogEvery)
}

// memoryArgument reads --max-memory=N, the heap at which the child ends itself. What is missing, is not a
// number or is under the least a child can work in is the default: the argument comes from the parent, which
// is this binary, and a mistake in it must not make the limit smaller than the process can live in.
func memoryArgument(args []string) uint64 {
	for _, a := range args {
		if v, ok := strings.CutPrefix(a, "--max-memory="); ok {
			if n, err := strconv.ParseUint(v, 10, 64); err == nil && n >= minChildMemory {
				return n
			}
		}
	}
	return defaultChildMemory
}

// startMemoryWatchdog ends the process when its heap is larger than hard bytes, with a failure the parent
// reads as a crash. It checks every interval; an allocation can overshoot between two checks, which is why
// the limit is well under what the host has.
func startMemoryWatchdog(hard uint64, every time.Duration) {
	go func() {
		var m runtime.MemStats
		for {
			time.Sleep(every)
			runtime.ReadMemStats(&m)
			if m.HeapAlloc > hard {
				os.Exit(childOutOfMemoryExit)
			}
		}
	}()
}

// preferToBeKilled tells a Linux kernel that is out of memory to choose this process before the service
// that started it. It does nothing where there is no /proc, and where the write is not allowed.
func preferToBeKilled() {
	_ = os.WriteFile("/proc/self/oom_score_adj", []byte("1000"), 0o644)
}

// RunPDFChild is the child's side of the protocol. Arguments are --max-pages=N and --max-input=N.
// It reads a PDF from stdin, up to the input limit, and writes one JSON object to stdout: the pages,
// or an error code (malformed, encrypted, too_large). It returns 0 for any answer it could give, and
// 2 only when it could not write it.
func RunPDFChild(args []string, stdin io.Reader, stdout io.Writer) int {
	maxPages, maxInput := 0, int64(defaultChildInput)
	for _, a := range args {
		switch {
		case strings.HasPrefix(a, "--max-pages="):
			if n, err := strconv.Atoi(strings.TrimPrefix(a, "--max-pages=")); err == nil && n > 0 && n <= maxChildPagesArgument {
				maxPages = n
			}
		case strings.HasPrefix(a, "--max-input="):
			if n, err := strconv.ParseInt(strings.TrimPrefix(a, "--max-input="), 10, 64); err == nil && n > 0 {
				maxInput = n
			}
		}
	}

	reply := func(r childReply) int {
		if err := json.NewEncoder(stdout).Encode(r); err != nil {
			return 2
		}
		return 0
	}
	data, err := io.ReadAll(io.LimitReader(stdin, maxInput+1))
	if err != nil {
		return reply(childReply{Error: childReplyMalformed})
	}
	if int64(len(data)) > maxInput {
		return reply(childReply{Error: childReplyTooLarge})
	}
	res, err := ExtractPDF(data, maxPages)
	switch {
	case errors.Is(err, ErrEncrypted):
		return reply(childReply{Error: childReplyEncrypted})
	case err != nil:
		return reply(childReply{Error: childReplyMalformed})
	}
	return reply(childReply{Pages: res.Pages, Total: res.TotalPages, Truncated: res.Truncated})
}
