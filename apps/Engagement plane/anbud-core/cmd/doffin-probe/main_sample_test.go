package main

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

const (
	sampleEU       = "eforms-sdk-1.14#extended#urn:fdc:anskaffelser.no:2023:eforms:eu"
	sampleNational = "eforms-sdk-1.8#extended#urn:fdc:anskaffelser.no:2023:eforms:national"
)

// sampleNotice is a downloadable eForms notice for id, on the given platform.
func sampleNotice(id, customization, host string) doffin.Notice {
	return doffin.Notice{ID: id, ContentType: "application/octet-stream", Body: []byte(eformsDoc(customization, host))}
}

// The corpus sample is a fixed plan of searches. Its summary is the thing the
// parser is built from, so every line of it is pinned here, not just its presence.
func TestSampleSummaryIsExact(t *testing.T) {
	t.Parallel()

	var asked []string
	client := &fakeNotices{
		search: func(p doffin.SearchParams) (doffin.SearchResponse, error) {
			typ := "ANY"
			if len(p.Types) > 0 {
				typ = p.Types[0]
			}
			asked = append(asked, fmt.Sprintf("%s:%d", typ, p.NumHitsPerPage))
			ids := []string{fmt.Sprintf("2026-%06d", len(asked)*100)}
			if p.NumHitsPerPage == 3 {
				ids = []string{"2026-000100", "2026-000101", "2026-000102"}
			}
			if p.SortBy == "PUBLICATION_DATE_ASC" {
				ids = []string{"2017-011757"}
			}
			return response(ids...), nil
		},
		download: func(id string) (doffin.Notice, error) {
			switch {
			case strings.HasPrefix(id, "2017-"):
				return doffin.Notice{ID: id, ContentType: "text/html", Body: []byte("<html>legacy</html>")}, nil
			case strings.HasSuffix(id, "1"):
				return sampleNotice(id, sampleNational, "kgv.example.no"), nil
			default:
				return sampleNotice(id, sampleEU, "permalink.mercell.com"), nil
			}
		},
	}

	dir := t.TempDir()
	var out bytes.Buffer
	p := &probe{out: &out, client: client, saveDir: dir}
	if err := p.sample(context.Background(), 3); err != nil {
		t.Fatalf("sample: %v", err)
	}

	wantAsked := strings.Join([]string{
		"ANNOUNCEMENT_OF_COMPETITION:3", "ADVISORY_NOTICE:1", "PRE_ANNOUNCEMENT:1", "NOTICE_ON_BUYER_PROFILE:1",
		"ANNOUNCEMENT_OF_INTENT:1", "ANNOUNCEMENT_OF_CONCLUSION_OF_CONTRACT:1", "CHANGE_OF_CONCLUSION_OF_CONTRACT:1",
		"CANCELLED_OR_MISSING_CONCLUSION_OF_CONTRACT:1", "DYNAMIC_PURCHASING_SCHEME:1", "QUALIFICATION_SCHEME:1", "ANY:1",
	}, ",")
	if got := strings.Join(asked, ","); got != wantAsked {
		t.Errorf("searches ran in the wrong order or size:\n got %s\nwant %s", got, wantAsked)
	}

	text := out.String()
	for _, want := range []string{
		"summary: 13 downloaded, 1 not eForms, 0 failed\n",
		"flavours: eu=11 national=1\n",
		"roots: ContractNotice=12\n",
		"sdk versions: 1.14=11 1.8=1\n",
		"subtypes: none=12\n",
		"platforms (hosts seen in the XML): permalink.mercell.com=11 kgv.example.no=1\n",
	} {
		if !strings.Contains(text, want) {
			t.Errorf("summary is missing %q:\n%s", want, text)
		}
	}
	if xmlFiles, _ := filepath.Glob(filepath.Join(dir, "sample-*.xml")); len(xmlFiles) != 12 {
		t.Errorf("saved %d XML notices, want 12", len(xmlFiles))
	}
	assertTerminalSafe(t, "output", text)
}

func TestSampleDownloadsEachNoticeOnce(t *testing.T) {
	t.Parallel()

	var downloads []string
	client := &fakeNotices{
		search: func(doffin.SearchParams) (doffin.SearchResponse, error) {
			return response("2026-000001"), nil // every step returns the same notice
		},
		download: func(id string) (doffin.Notice, error) {
			downloads = append(downloads, id)
			return sampleNotice(id, sampleEU, "permalink.mercell.com"), nil
		},
	}
	var out bytes.Buffer
	if err := (&probe{out: &out, client: client}).sample(context.Background(), 3); err != nil {
		t.Fatalf("sample: %v", err)
	}
	if len(downloads) != 1 {
		t.Errorf("downloaded %v, want the one notice exactly once", downloads)
	}
	if !strings.Contains(out.String(), "summary: 1 downloaded, 0 not eForms, 0 failed") {
		t.Errorf("summary:\n%s", out.String())
	}
}

// A rate limit affects every later call, so the sample stops. What it learned
// before then is still reported, and the error still reaches the caller.
func TestSampleStopsAtARateLimitAndStillSummarises(t *testing.T) {
	t.Parallel()

	calls, downloaded := 0, 0
	client := &fakeNotices{
		search: func(doffin.SearchParams) (doffin.SearchResponse, error) {
			calls++
			if calls == 2 {
				return doffin.SearchResponse{}, &doffin.RateLimitedError{Operation: "search", RetryAfter: 90 * time.Second}
			}
			return response("2026-000100", "2026-000101", "2026-000102"), nil
		},
		download: func(id string) (doffin.Notice, error) {
			downloaded++
			return sampleNotice(id, sampleEU, "permalink.mercell.com"), nil
		},
	}
	var out bytes.Buffer
	err := (&probe{out: &out, client: client}).sample(context.Background(), 3)

	var rateErr *doffin.RateLimitedError
	if !errors.As(err, &rateErr) {
		t.Fatalf("error = %v, want the rate-limit error", err)
	}
	if calls != 2 || downloaded != 3 {
		t.Errorf("calls=%d downloaded=%d, want the sample to stop after the second search with 3 downloads", calls, downloaded)
	}
	for _, want := range []string{"stopped early:", "summary: 3 downloaded, 0 not eForms, 0 failed"} {
		if !strings.Contains(out.String(), want) {
			t.Errorf("output should contain %q:\n%s", want, out.String())
		}
	}
}

func TestSampleStopsWhenTheTimeLimitHasEnded(t *testing.T) {
	t.Parallel()

	client := &fakeNotices{search: func(doffin.SearchParams) (doffin.SearchResponse, error) {
		t.Error("no search may run once the time limit has ended")
		return doffin.SearchResponse{}, nil
	}}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()

	var out bytes.Buffer
	err := (&probe{out: &out, client: client}).sample(ctx, 3)
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("error = %v, want context.Canceled", err)
	}
	if !strings.Contains(out.String(), "summary: 0 downloaded, 0 not eForms, 0 failed") {
		t.Errorf("the summary should still print:\n%s", out.String())
	}
}

// One notice that fails to download is counted and skipped; it does not end the
// sample, and the notices after it are still fetched.
func TestSampleCountsAFailedDownloadAndCarriesOn(t *testing.T) {
	t.Parallel()

	calls := 0
	var downloads []string
	client := &fakeNotices{
		search: func(doffin.SearchParams) (doffin.SearchResponse, error) {
			calls++
			if calls == 1 {
				return response("2026-000100", "2026-000101", "2026-000102"), nil
			}
			return response(), nil
		},
		download: func(id string) (doffin.Notice, error) {
			downloads = append(downloads, id)
			if id == "2026-000101" {
				return doffin.Notice{}, &doffin.HTTPError{Operation: "download", Status: 404}
			}
			return sampleNotice(id, sampleEU, "permalink.mercell.com"), nil
		},
	}
	var out bytes.Buffer
	if err := (&probe{out: &out, client: client}).sample(context.Background(), 3); err != nil {
		t.Fatalf("a failed download must not end the sample: %v", err)
	}
	if got := strings.Join(downloads, ","); got != "2026-000100,2026-000101,2026-000102" {
		t.Errorf("downloads = %s; the sample must go on after a failure", got)
	}
	if !strings.Contains(out.String(), "summary: 2 downloaded, 0 not eForms, 1 failed") {
		t.Errorf("summary:\n%s", out.String())
	}
}

// When the typed decode fails, every id in the raw body is a candidate, so a
// shape the spec gets wrong still yields a sample. The sample never takes more
// than it asked for, however many hits the body holds.
func TestSampleUsesTheIdsInARawBodyButNoMoreThanAsked(t *testing.T) {
	t.Parallel()

	raw := []byte(`{"numHitsTotal":5,"hits":[` +
		`{"id":"2026-000001","heading":{"surprise":true}},` +
		`{"id":"2026-000002","heading":{"surprise":true}},` +
		`{"id":"2026-000003","heading":{"surprise":true}},` +
		`{"id":"2026-000004","heading":{"surprise":true}},` +
		`{"id":"2026-000005","heading":{"surprise":true}}]}`)
	calls := 0
	var downloads []string
	client := &fakeNotices{
		search: func(doffin.SearchParams) (doffin.SearchResponse, error) {
			calls++
			if calls == 1 {
				return doffin.SearchResponse{Raw: raw}, &doffin.DecodeError{Operation: "search", Err: errors.New("shape")}
			}
			return response(), nil
		},
		download: func(id string) (doffin.Notice, error) {
			downloads = append(downloads, id)
			return sampleNotice(id, sampleEU, "permalink.mercell.com"), nil
		},
	}
	var out bytes.Buffer
	if err := (&probe{out: &out, client: client}).sample(context.Background(), 3); err != nil {
		t.Fatalf("sample: %v", err)
	}
	if got := strings.Join(downloads, ","); got != "2026-000001,2026-000002,2026-000003" {
		t.Errorf("downloads = %s, want the first three ids of the raw body", got)
	}
	if !strings.Contains(out.String(), "using the ids from the raw response") {
		t.Errorf("the fallback should be announced:\n%s", out.String())
	}
}

func TestPrintCountsIsBoundedAndNamesAnAbsentValue(t *testing.T) {
	t.Parallel()

	counts := map[string]int{}
	for i := range 25 {
		counts[fmt.Sprintf("k%02d", i)] = i + 1
	}
	var out bytes.Buffer
	(&probe{out: &out}).printCounts("roots", counts, 20)
	line := strings.TrimSpace(out.String())
	if n := strings.Count(line, "="); n != 20 {
		t.Errorf("shown %d entries, want 20: %s", n, line)
	}
	if !strings.HasSuffix(line, "and 5 more") {
		t.Errorf("the line should say how many were left out: %s", line)
	}

	if valueOrNone("", 10) != "none" || valueOrNone("  ", 10) != "none" || valueOrNone("x", 10) != "x" {
		t.Error("valueOrNone should name an absent value and keep a present one")
	}
}

func TestURLHostsAreBounded(t *testing.T) {
	t.Parallel()

	var b strings.Builder
	for i := range 30 {
		fmt.Fprintf(&b, `<u>https://h%02d.example.no/x</u>`, i)
	}
	if got := urlHosts([]byte(b.String())); len(got) != maxHostsPerNotice {
		t.Errorf("collected %d hosts, want at most %d", len(got), maxHostsPerNotice)
	}
}
