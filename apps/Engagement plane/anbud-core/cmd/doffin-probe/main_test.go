package main

import (
	"bytes"
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"unicode"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// fakeNotices stands in for the real client, so the probe's flow is tested
// without pacing, a network, or a key.
type fakeNotices struct {
	search   func(doffin.SearchParams) (doffin.SearchResponse, error)
	download func(string) (doffin.Notice, error)
}

func (f *fakeNotices) Search(_ context.Context, p doffin.SearchParams) (doffin.SearchResponse, error) {
	return f.search(p)
}

func (f *fakeNotices) Download(_ context.Context, id string) (doffin.Notice, error) {
	if f.download == nil {
		return doffin.Notice{}, errors.New("download not expected")
	}
	return f.download(id)
}

func (f *fakeNotices) Budgets() doffin.Budgets { return doffin.Budgets{SearchRPM: 24, DownloadRPM: 8} }

// unsafeRune reports anything a terminal or a reader could be tricked by:
// control characters (including ESC and BEL) and invisible format characters
// such as bidi overrides and zero-width spaces.
func unsafeRune(r rune) bool { return unicode.IsControl(r) || unicode.Is(unicode.Cf, r) }

func assertTerminalSafe(t *testing.T, label, s string) {
	t.Helper()
	for _, r := range s {
		if r == '\n' && label == "output" {
			continue // the probe's own line breaks
		}
		if unsafeRune(r) {
			t.Fatalf("%s contains %U: %q", label, r, s)
		}
	}
}

func hit(id string) doffin.Hit { return doffin.Hit{ID: id, Type: "COMPETITION", Heading: "heading"} }

func strPtr(s string) *string { return &s }

func response(ids ...string) doffin.SearchResponse {
	resp := doffin.SearchResponse{NumHitsTotal: 100, NumHitsAccessible: 100}
	for _, id := range ids {
		resp.Hits = append(resp.Hits, hit(id))
	}
	return resp
}

func TestPrintableRemovesEverythingATerminalOrAReaderCouldBeTrickedBy(t *testing.T) {
	t.Parallel()

	in := "ok\x1b[31mred\x1b[0m\x07\n\t \u202eevil\u202c \u200bzero\u2028sep"
	out := printable(in, 200)

	assertTerminalSafe(t, "printable output", out)
	for _, want := range []string{"ok", "red", "evil", "zero", "sep"} {
		if !strings.Contains(out, want) {
			t.Errorf("printable dropped visible text %q: %q", want, out)
		}
	}
	if got := printable(strings.Repeat("x", 100), 10); got != strings.Repeat("x", 10)+"…" {
		t.Errorf("truncation = %q", got)
	}
}

func TestDescribeSanitisesEveryUpstreamField(t *testing.T) {
	t.Parallel()

	h := doffin.Hit{
		ID:              "2025-1\x1b[2J",
		Type:            "COMP\u202eETITION",
		PublicationDate: "2025\n-12",
		Heading:         "H\u200bi\x1b]0;pwn\x07",
		Status:          strPtr("\x1b[31mACTIVE"),
		Deadline:        strPtr("2026\u202e-01"),
		Buyer:           doffin.OneOrMany[doffin.Organization]{{Name: "\x1b]0;pwn\x07Buyer"}},
	}
	assertTerminalSafe(t, "describe output", describe(h))
}

func TestWriteFileAtomicIsPrivateAndDoesNotFollowSymlinks(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	outside := filepath.Join(t.TempDir(), "outside.txt")
	if err := os.WriteFile(outside, []byte("keep"), 0o600); err != nil {
		t.Fatal(err)
	}
	target := filepath.Join(dir, "search-baseline.json")
	if err := os.Symlink(outside, target); err != nil {
		t.Skipf("symlinks unavailable: %v", err)
	}

	path, err := writeFileAtomic(dir, "search-baseline.json", []byte("new"))
	if err != nil {
		t.Fatalf("writeFileAtomic: %v", err)
	}
	if path != target {
		t.Errorf("path = %q, want %q", path, target)
	}
	if got, _ := os.ReadFile(outside); string(got) != "keep" {
		t.Fatalf("a planted symlink was followed: the file it pointed at now holds %q", got)
	}
	info, err := os.Lstat(target)
	if err != nil {
		t.Fatal(err)
	}
	if info.Mode()&os.ModeSymlink != 0 || info.Mode().Perm() != 0o600 {
		t.Errorf("target mode = %v, want a regular file with 0600", info.Mode())
	}
	if got, _ := os.ReadFile(target); string(got) != "new" {
		t.Errorf("target holds %q", got)
	}
	leftovers, _ := filepath.Glob(filepath.Join(dir, ".probe-*"))
	if len(leftovers) != 0 {
		t.Errorf("temporary files left behind: %v", leftovers)
	}
}

func TestWriteFileAtomicCreatesAPrivateDirectory(t *testing.T) {
	t.Parallel()

	dir := filepath.Join(t.TempDir(), "captured")
	if _, err := writeFileAtomic(dir, "a.json", []byte("x")); err != nil {
		t.Fatalf("writeFileAtomic: %v", err)
	}
	info, err := os.Stat(dir)
	if err != nil {
		t.Fatal(err)
	}
	if info.Mode().Perm()&0o077 != 0 {
		t.Errorf("directory mode = %v, want no group or world access", info.Mode().Perm())
	}
}

func TestWriteFileAtomicRejectsUnsafeNames(t *testing.T) {
	t.Parallel()

	for _, name := range []string{"", ".", "..", "../x", "a/b", `a\b`, ".hidden"} {
		if _, err := writeFileAtomic(t.TempDir(), name, []byte("x")); err == nil {
			t.Errorf("name %q was accepted", name)
		}
	}
}

// The probe exists to discover shapes the spec gets wrong. A body that does not
// decode must be saved, and the probe must carry on instead of declaring the
// key broken.
func TestProbeKeepsTheRawBodyWhenDecodingFails(t *testing.T) {
	t.Parallel()

	raw := []byte(`{"numHitsTotal":1,"hits":[{"id":"2025-104837","heading":{"surprise":true}}]}`)
	client := &fakeNotices{
		search: func(doffin.SearchParams) (doffin.SearchResponse, error) {
			return doffin.SearchResponse{Raw: raw}, &doffin.DecodeError{Operation: "search", Err: errors.New("json: cannot unmarshal object into Go struct field Hit.heading of type string")}
		},
		download: func(id string) (doffin.Notice, error) {
			return doffin.Notice{ID: id, ContentType: "application/xml", Body: []byte("<n/>"), SHA256: "abc"}, nil
		},
	}
	dir := t.TempDir()
	var out bytes.Buffer
	p := &probe{out: &out, client: client, saveDir: dir}

	if err := p.run(context.Background(), options{discover: true}); err != nil {
		t.Fatalf("run: %v", err)
	}

	saved, err := os.ReadFile(filepath.Join(dir, "search-baseline.json"))
	if err != nil {
		t.Fatalf("the raw body was not saved: %v", err)
	}
	if !bytes.Equal(saved, raw) {
		t.Errorf("saved body differs from the raw response: %q", saved)
	}
	text := out.String()
	if !strings.Contains(text, "does not match the published spec") {
		t.Errorf("the probe should say the shapes differ, not that the key is broken:\n%s", text)
	}
	if !strings.Contains(text, "download 2025-104837") {
		t.Errorf("the id should be recovered from the raw body so download can still be tested:\n%s", text)
	}
	assertTerminalSafe(t, "output", text)
}

func TestProbeStopsWhenTheBaselineIsRejected(t *testing.T) {
	t.Parallel()

	client := &fakeNotices{search: func(doffin.SearchParams) (doffin.SearchResponse, error) {
		return doffin.SearchResponse{}, &doffin.HTTPError{Operation: "search", Status: 401, Message: "Access denied"}
	}}
	p := &probe{out: &bytes.Buffer{}, client: client}

	err := p.run(context.Background(), options{discover: true, skipDownload: true})
	if err == nil || !strings.Contains(err.Error(), "key or the connection") {
		t.Fatalf("error = %v, want one that points at the key or connection", err)
	}
	if !errors.Is(err, doffin.ErrUnauthorized) {
		t.Errorf("the underlying cause should stay inspectable: %v", err)
	}
}

func TestProbeInfersThePagingBase(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		page func(n int) (doffin.SearchResponse, error)
		want string
	}{
		{"page 0 is clamped to the first page", func(n int) (doffin.SearchResponse, error) {
			return response([]string{"A", "A", "B"}[n]), nil
		}, "1-based"},
		{"page 0 is rejected", func(n int) (doffin.SearchResponse, error) {
			if n == 0 {
				return doffin.SearchResponse{}, &doffin.HTTPError{Operation: "search", Status: 400}
			}
			return response([]string{"", "A", "B"}[n]), nil
		}, "1-based"},
		{"every page differs", func(n int) (doffin.SearchResponse, error) {
			return response([]string{"A", "B", "C"}[n]), nil
		}, "0-based"},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			client := &fakeNotices{search: func(p doffin.SearchParams) (doffin.SearchResponse, error) {
				if p.Page != nil {
					return tc.page(*p.Page)
				}
				return response("2025-100000"), nil
			}}
			var out bytes.Buffer
			p := &probe{out: &out, client: client}
			if err := p.run(context.Background(), options{discover: true, skipDownload: true}); err != nil {
				t.Fatalf("run: %v", err)
			}
			if !strings.Contains(out.String(), tc.want) {
				t.Fatalf("expected the output to conclude %q:\n%s", tc.want, out.String())
			}
		})
	}
}

func TestRunRequiresAKey(t *testing.T) {
	t.Setenv("DOFFIN_API_KEY", "")

	err := run(nil, &bytes.Buffer{})
	if err == nil || !strings.Contains(err.Error(), "DOFFIN_API_KEY") {
		t.Fatalf("error = %v, want one that names DOFFIN_API_KEY", err)
	}
}

func TestRunRejectsAMalformedKeyWithoutEchoingIt(t *testing.T) {
	const bad = "short key"
	t.Setenv("DOFFIN_API_KEY", bad)

	err := run(nil, &bytes.Buffer{})
	if err == nil {
		t.Fatal("expected an error")
	}
	if strings.Contains(err.Error(), bad) {
		t.Fatalf("the error echoed the key: %v", err)
	}
	if !strings.Contains(err.Error(), "DOFFIN_") {
		t.Errorf("the error should say which settings to check: %v", err)
	}
}
