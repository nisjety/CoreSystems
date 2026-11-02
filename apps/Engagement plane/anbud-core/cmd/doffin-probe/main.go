// Command doffin-probe verifies Doffin Public API subscription keys with a
// handful of read-only calls and settles what the published OpenAPI document
// leaves open: the real response shapes, which sortBy values work, whether
// paging starts at 0 or 1, and what a downloaded notice looks like.
//
// It never prints a key. Everything the API sends is passed through printable
// before it reaches the terminal. Nothing is written to disk unless -save-dir is
// given, and every raw body is saved even when it does not decode, because that
// is exactly the evidence needed to correct the types. Calls are paced under the
// per-key budgets.
//
// Those budgets are shared with doffin-sync, which holds a Postgres advisory lock
// so that one process spends them. When ANBUD_DSN is set, the probe takes the same
// lock before its first call, holds it for its whole run and releases it on every
// exit. If another sync holds it, the probe calls nothing, says so and exits 7, the
// code doffin-sync uses for the same condition. A DSN for a host that is not
// loopback must verify the server certificate, the rule doffin-sync applies. So must
// the role it names be one that cannot drop the append-only triggers (docs/ROLES.md): a
// superuser, a role with CREATEROLE, CREATEDB or BYPASSRLS, or one that owns tables is
// refused off loopback, with exit 1 and no call made, and on loopback it is one warning
// line on stderr and the run goes on. When
// ANBUD_DSN is unset the probe takes no lock, says so in its header, and must not
// run while a sync does. It applies no migration and writes nothing to the database.
//
//	set -a; source .env; set +a
//	go run ./cmd/doffin-probe -save-dir testdata/captured                    # keys + shape discovery
//	go run ./cmd/doffin-probe -discover=false -samples 12 -save-dir testdata/captured   # keys + corpus sample
package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"time"
	"unicode"

	"github.com/triodelab/anbud/internal/config"
	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/secret"
	"github.com/triodelab/anbud/internal/sources/doffin"
)

func main() {
	os.Exit(finish(run(os.Args[1:], os.Stdout), os.Stderr))
}

// notices is the part of the Doffin client the probe uses, so its flow can be
// tested without pacing, a network or a key.
type notices interface {
	Search(ctx context.Context, params doffin.SearchParams) (doffin.SearchResponse, error)
	Download(ctx context.Context, doffinID string) (doffin.Notice, error)
	Budgets() doffin.Budgets
}

// keyedClient is a client bound to one of the subscription's keys.
type keyedClient struct {
	label  string
	client notices
}

type options struct {
	discover     bool // the shape-discovery steps [1]-[6]
	skipDownload bool // skip the single notice download in step [6]
	samples      int  // >0: also fetch a corpus sample, this many recent competitions plus one of every other type
}

// maxSummaryRows bounds each line of the sample summary, and maxHostsPerNotice
// the platform hosts read from one notice. Both bound what a hostile or unusual
// notice can add to the output and to memory.
const (
	maxSummaryRows    = 20
	maxHostsPerNotice = 20
)

func run(args []string, out io.Writer) error {
	flags := flag.NewFlagSet("doffin-probe", flag.ContinueOnError)
	saveDir := flags.String("save-dir", "", "write raw responses here (testdata/captured is gitignored)")
	skipDownload := flags.Bool("skip-download", false, "do not download a notice in the discovery steps")
	discover := flags.Bool("discover", true, "run the shape-discovery steps")
	samples := flags.Int("samples", 0, "also download a corpus sample: this many recent competition notices plus one of every other type")
	timeout := flags.Duration("timeout", 0, "overall time limit; 0 scales with -samples")
	if err := flags.Parse(args); err != nil {
		return err
	}
	// "-discover false" leaves "false" behind as an argument and would still run
	// the discovery steps, so it is refused rather than guessed at.
	if flags.NArg() > 0 {
		return fmt.Errorf("unexpected argument %q: write -discover=false, not -discover false", flags.Arg(0))
	}
	if *samples < 0 || *samples > doffin.MaxHitsPerPage {
		return fmt.Errorf("-samples must be between 0 and %d", doffin.MaxHitsPerPage)
	}
	if *timeout < 0 {
		return errors.New("-timeout must not be negative")
	}
	if *timeout == 0 {
		*timeout = probeTimeout(*samples)
	}

	cfg, err := config.LoadDoffin()
	if err != nil {
		return err
	}
	if !cfg.Configured() {
		return errors.New("DOFFIN_API_KEY is not set; add it to .env, then run: set -a; source .env; set +a")
	}

	const hint = "check DOFFIN_API_KEY, DOFFIN_API_KEY_SECONDARY, DOFFIN_BASE_URL, DOFFIN_SEARCH_RPM, DOFFIN_DOWNLOAD_RPM"
	primary, err := newClient(cfg, cfg.APIKey)
	if err != nil {
		return fmt.Errorf("invalid Doffin settings (%s): %w", hint, err)
	}
	keys := []keyedClient{{"primary", primary}}
	// A secondary key identical to the primary is not a second key, so it is
	// neither added nor checked again; checkKeys says so.
	sameKey := cfg.SecondaryAPIKey.IsSet() && cfg.SecondaryAPIKey.Reveal() == cfg.APIKey.Reveal()
	if cfg.SecondaryAPIKey.IsSet() && !sameKey {
		secondary, err := newClient(cfg, cfg.SecondaryAPIKey)
		if err != nil {
			return fmt.Errorf("invalid Doffin settings (%s): %w", hint, err)
		}
		keys = append(keys, keyedClient{"secondary", secondary})
	}

	// The budget lock is shared with doffin-sync. A DSN that is refused stops the
	// probe here, before any connection and before any Doffin call.
	locker, err := lockFromEnv()
	if err != nil {
		return err
	}

	ctx, cancel := context.WithTimeout(context.Background(), *timeout)
	defer cancel()
	p := &probe{out: out, keys: keys, sameKey: sameKey, saveDir: *saveDir, host: hostOf(cfg.BaseURL), locker: locker}
	return p.run(ctx, options{discover: *discover, skipDownload: *skipDownload, samples: *samples})
}

// probeTimeout is the time limit when -timeout is not given. The discovery steps
// take a few minutes. Each download is paced to about 7.5 seconds at the default
// of 8 a minute, so 15 seconds per notice leaves room for transfers and back-offs.
func probeTimeout(samples int) time.Duration {
	return 5*time.Minute + time.Duration(samples+10)*15*time.Second
}

// hostOf names the host the keys are sent to, so the operator can see it.
func hostOf(rawURL string) string {
	u, err := url.Parse(rawURL)
	if err != nil {
		return ""
	}
	return printable(u.Hostname(), 80)
}

func newClient(cfg config.Doffin, key secret.Value) (*doffin.Client, error) {
	return doffin.New(doffin.Config{
		BaseURL:     cfg.BaseURL,
		APIKey:      key,
		SearchRPM:   cfg.SearchRPM,
		DownloadRPM: cfg.DownloadRPM,
		Timeout:     cfg.HTTPTimeout,
	})
}

type probe struct {
	out      io.Writer
	client   notices       // the client the steps use; chosen from keys by checkKeys
	keys     []keyedClient // primary first; empty when client is set directly
	sameKey  bool          // the secondary key is a copy of the primary
	host     string        // the host the keys are sent to
	saveDir  string
	locker   budgetLocker // the Doffin budget lock; nil when ANBUD_DSN is not set, and then no lock is taken
	lockHeld bool         // run holds the budget lock; set by run, read by the header
}

func (p *probe) printf(format string, args ...any) { fmt.Fprintf(p.out, format, args...) }

func (p *probe) run(ctx context.Context, opts options) error {
	if p.locker != nil {
		// The lock comes before the header and before every Doffin call, and is held
		// until run returns, on every path. A lock held elsewhere ends the run here.
		if err := p.locker.TryLock(ctx); err != nil {
			return lockFailure(err)
		}
		p.lockHeld = true
		defer p.releaseLock(ctx)
	}
	p.header()
	if len(p.keys) > 0 {
		if err := p.checkKeys(ctx); err != nil {
			return err
		}
	}
	if opts.discover {
		if err := p.discover(ctx, opts.skipDownload); err != nil {
			return err
		}
	}
	if opts.samples > 0 {
		if err := p.sample(ctx, opts.samples); err != nil {
			return err
		}
	}
	p.printf("\nDone. Paste this output back so the parser can be built against the real shapes.\n")
	return nil
}

func (p *probe) header() {
	var budgets doffin.Budgets
	switch {
	case p.client != nil:
		budgets = p.client.Budgets()
	case len(p.keys) > 0:
		budgets = p.keys[0].client.Budgets()
	}
	p.printf("Doffin Public API probe  %s\n", time.Now().UTC().Format(time.RFC3339))
	if p.host != "" {
		p.printf("host: %s\n", p.host)
	}
	p.printf("budgets: search %d/min, download %d/min   keys: never printed\n", budgets.SearchRPM, budgets.DownloadRPM)
	p.printf("%s\n\n", p.lockLine())
}

// checkKeys runs one search with each key and carries on with the first that
// works. Only a rejection counts against a key. A rate limit or any other failure
// (an outage, a dropped connection, an expired time limit) says nothing about the
// key, so it stops the probe instead of sending the owner to fix a key that is fine.
func (p *probe) checkKeys(ctx context.Context) error {
	p.printf("[0] keys (one search each)\n")
	chosen := -1
	for i, k := range p.keys {
		resp, err := k.client.Search(ctx, doffin.SearchParams{NumHitsPerPage: 1})
		var (
			decodeErr *doffin.DecodeError
			rateErr   *doffin.RateLimitedError
		)
		switch {
		case err == nil:
			p.printf("  %-10s accepted (numHitsTotal=%d)\n", k.label, resp.NumHitsTotal)
		case errors.As(err, &decodeErr):
			p.printf("  %-10s accepted (the response shape differs from the spec)\n", k.label)
		case errors.As(err, &rateErr):
			return fmt.Errorf("the %s key could not be checked because Doffin is rate limiting this subscription: %w", k.label, err)
		case errors.Is(err, doffin.ErrUnauthorized):
			p.printf("  %-10s REJECTED: %s\n", k.label, errText(err))
			continue
		default:
			return fmt.Errorf("the %s key could not be checked, so nothing can be concluded about it: %w", k.label, err)
		}
		if chosen < 0 {
			chosen = i
		}
	}
	if p.sameKey {
		p.printf("  note: the secondary key is identical to the primary, so it adds no rotation safety\n")
	}
	if chosen < 0 {
		return errors.New("neither key was accepted: use the Primary and Secondary keys of your subscription to the Public API on the production portal, not the Notices/eForm API and not the test portal")
	}
	if chosen > 0 {
		p.printf("  => the primary was rejected but the secondary works: continuing with the secondary; swap the two in .env and regenerate the rejected one\n")
	}
	p.client = p.keys[chosen].client
	p.printf("\n")
	return nil
}

func (p *probe) discover(ctx context.Context, skipDownload bool) error {
	// 1. Baseline: proves the key works and shows the real shape of a hit.
	p.printf("[1] baseline search (numHitsPerPage=3)\n")
	baseline, err := p.search(ctx, "baseline", doffin.SearchParams{NumHitsPerPage: 3})
	var decodeErr *doffin.DecodeError
	switch {
	case errors.As(err, &decodeErr):
		p.printf("  ! the key works, but the response does not match the published spec; the raw body is saved when -save-dir is set\n")
	case err != nil:
		return fmt.Errorf("the baseline search failed, so the key or the connection is not working: %w", err)
	}
	downloadID := firstID(baseline)

	// 2. Which notice types carry live tenders, and how many are reachable?
	p.printf("\n[2] notice type x status\n")
	for _, c := range []struct {
		label    string
		types    []string
		statuses []string
	}{
		{"competition-active", []string{"COMPETITION"}, []string{"ACTIVE"}},
		{"announcement-of-competition-active", []string{"ANNOUNCEMENT_OF_COMPETITION"}, []string{"ACTIVE"}},
		{"result-awarded", []string{"RESULT"}, []string{"AWARDED"}},
	} {
		p.printf("  %s\n", c.label)
		resp, err := p.search(ctx, c.label, doffin.SearchParams{Types: c.types, Statuses: c.statuses, NumHitsPerPage: 2})
		// Prefer a live competition notice for the download.
		if id := firstID(resp); err == nil && id != "" && strings.HasPrefix(c.label, "competition") {
			downloadID = id
		}
	}

	// 3. sortBy: the description text and the OpenAPI enum list different values.
	p.printf("\n[3] sortBy (the description and the OpenAPI enum disagree)\n")
	for _, sortBy := range []string{
		"PUBLICATION_DATE_DESC", "PUBLICATION_DATE_ASC", "ISSUE_DATE_DESC", "ISSUE_DATE_ASC",
		"DEADLINE", "RELEVANCE", "ESTIMATED_VALUE_DESC",
	} {
		label := "sort-" + strings.ToLower(strings.ReplaceAll(sortBy, "_", "-"))
		resp, err := p.fetch(ctx, label, doffin.SearchParams{SortBy: sortBy, NumHitsPerPage: 1})
		if err != nil && !errors.As(err, &decodeErr) {
			p.printf("  %-24s rejected: %s\n", sortBy, errText(err))
			continue
		}
		p.printf("  %-24s ok (first: %s)\n", sortBy, printable(firstID(resp), 24))
	}

	// 4. Paging base: 0-based or 1-based?
	p.printf("\n[4] paging base (numHitsPerPage=1, default sort)\n")
	ids := make(map[int]string)
	for page := 0; page <= 2; page++ {
		n := page
		resp, err := p.fetch(ctx, fmt.Sprintf("page-%d", page), doffin.SearchParams{Page: &n, NumHitsPerPage: 1})
		if err != nil && !errors.As(err, &decodeErr) {
			p.printf("  page=%d rejected: %s\n", page, errText(err))
			continue
		}
		ids[page] = firstID(resp)
		p.printf("  page=%d -> %s\n", page, printable(ids[page], 24))
	}
	switch {
	case ids[0] == "" && ids[1] != "":
		p.printf("  => page 0 is rejected: paging looks 1-based\n")
	case ids[0] != "" && ids[0] == ids[1]:
		p.printf("  => pages 0 and 1 return the same notice: paging looks 1-based (0 is clamped)\n")
	case ids[0] != "" && ids[1] != "" && ids[0] != ids[1]:
		p.printf("  => pages 0 and 1 differ: paging looks 0-based (re-run if a notice was published meanwhile)\n")
	}

	// 5. The documented page-size ceiling.
	p.printf("\n[5] page size 100\n")
	if resp, err := p.search(ctx, "page-size-100", doffin.SearchParams{NumHitsPerPage: 100}); err == nil {
		p.printf("  returned %d hits, %d KiB of JSON\n", len(resp.Hits), len(resp.Raw)/1024)
	}

	// 6. What does a downloaded notice look like?
	if skipDownload || downloadID == "" {
		p.printf("\n[6] download skipped\n")
		return nil
	}
	p.printf("\n[6] download %s\n", printable(downloadID, 24))
	notice, err := p.client.Download(ctx, downloadID)
	if err != nil {
		p.printf("  rejected: %s\n", errText(err))
		return nil
	}
	p.printf("  content-type=%q size=%d bytes sha256=%s\n", notice.ContentType, len(notice.Body), notice.SHA256)
	p.printf("  starts with: %s\n", printable(string(notice.Body), 160))
	p.save("download-"+notice.ID+extensionFor(notice.ContentType, notice.Body), notice.Body)
	return nil
}

// sampleRef is a notice picked for the corpus sample.
type sampleRef struct{ id, apiType string }

// sampleStep is one search of the corpus sample.
type sampleStep struct {
	label  string
	params doffin.SearchParams
}

// samplePlan is the searches behind the corpus sample: recent competitions, one
// notice of every other type, and the oldest notice.
func samplePlan(recent int) []sampleStep {
	plan := []sampleStep{{"competition", doffin.SearchParams{Types: []string{"ANNOUNCEMENT_OF_COMPETITION"}, NumHitsPerPage: recent}}}
	for _, typ := range []string{
		"ADVISORY_NOTICE", "PRE_ANNOUNCEMENT", "NOTICE_ON_BUYER_PROFILE", "ANNOUNCEMENT_OF_INTENT",
		"ANNOUNCEMENT_OF_CONCLUSION_OF_CONTRACT", "CHANGE_OF_CONCLUSION_OF_CONTRACT",
		"CANCELLED_OR_MISSING_CONCLUSION_OF_CONTRACT", "DYNAMIC_PURCHASING_SCHEME", "QUALIFICATION_SCHEME",
	} {
		plan = append(plan, sampleStep{strings.ToLower(strings.ReplaceAll(typ, "_", "-")), doffin.SearchParams{Types: []string{typ}, NumHitsPerPage: 1}})
	}
	return append(plan, sampleStep{"oldest", doffin.SearchParams{SortBy: "PUBLICATION_DATE_ASC", NumHitsPerPage: 1}})
}

// sampleStats is what the sample teaches about the corpus as a whole.
type sampleStats struct {
	downloaded, notEForms, failed int
	flavours, roots, sdks         map[string]int
	subtypes, platforms           map[string]int
}

func newSampleStats() *sampleStats {
	return &sampleStats{
		flavours: map[string]int{}, roots: map[string]int{}, sdks: map[string]int{},
		subtypes: map[string]int{}, platforms: map[string]int{},
	}
}

// sample downloads a spread of notices and reports, for each, what its header
// says; then summarises the flavours, document roots and tender platforms seen.
// It is how the parser's input space is measured instead of guessed. A rate
// limit or the time limit ends it early, and the summary still prints, so what
// was learned is not lost.
func (p *probe) sample(ctx context.Context, recent int) error {
	p.printf("\n[7] corpus sample (%d recent competitions, one of each other type, and the oldest notice)\n", recent)

	stats := newSampleStats()
	err := p.sampleSteps(ctx, samplePlan(recent), stats)
	if err != nil {
		p.printf("\n  stopped early: %s\n", errText(err))
	}
	p.printf("\n  summary: %d downloaded, %d not eForms, %d failed\n", stats.downloaded, stats.notEForms, stats.failed)
	p.printCounts("flavours", stats.flavours, maxSummaryRows)
	p.printCounts("roots", stats.roots, maxSummaryRows)
	p.printCounts("sdk versions", stats.sdks, maxSummaryRows)
	p.printCounts("subtypes", stats.subtypes, maxSummaryRows)
	p.printCounts("platforms (hosts seen in the XML)", stats.platforms, maxSummaryRows)
	return err
}

// sampleSteps runs the plan in order. A notice that appears in more than one
// result is downloaded once. Only an error that affects every later call ends
// the sample; any other failure is counted and the sample carries on.
func (p *probe) sampleSteps(ctx context.Context, plan []sampleStep, stats *sampleStats) error {
	seen := map[string]bool{}
	for _, s := range plan {
		if err := ctx.Err(); err != nil {
			return err
		}
		resp, err := p.fetch(ctx, "sample-"+s.label, s.params)
		var decodeErr *doffin.DecodeError
		if err != nil && !errors.As(err, &decodeErr) {
			if stopsSample(ctx, err) {
				return err
			}
			p.printf("  %s: search failed: %s\n", s.label, errText(err))
			continue
		}
		refs, fromRaw := refsOf(resp)
		if fromRaw {
			p.printf("  %s: the typed decode failed; using the ids from the raw response\n", s.label)
		}
		refs = capRefs(refs, s.params.NumHitsPerPage)
		if len(refs) == 0 {
			p.printf("  %s: no notices\n", s.label)
			continue
		}
		for _, ref := range refs {
			if seen[ref.id] {
				continue
			}
			seen[ref.id] = true
			if err := p.sampleOne(ctx, s.label, ref, stats); err != nil {
				return err
			}
		}
	}
	return nil
}

// stopsSample reports whether an error should end the whole sample rather than
// skip one notice: a rate limit or an expired time limit affects every later call.
func stopsSample(ctx context.Context, err error) bool {
	var rateErr *doffin.RateLimitedError
	return ctx.Err() != nil || errors.As(err, &rateErr)
}

func (p *probe) sampleOne(ctx context.Context, label string, ref sampleRef, stats *sampleStats) error {
	id := printable(ref.id, 24)
	notice, err := p.client.Download(ctx, ref.id)
	if err != nil {
		if stopsSample(ctx, err) {
			return err
		}
		stats.failed++
		p.printf("  %s | %s | download failed: %s\n", id, printable(ref.apiType, 40), errText(err))
		return nil
	}
	stats.downloaded++
	p.save("sample-"+label+"-"+notice.ID+extensionFor(notice.ContentType, notice.Body), notice.Body)

	header, err := eforms.Sniff(notice.Body)
	if err != nil {
		stats.notEForms++
		p.printf("  %s | %s | not an eForms notice (%s, %d bytes): %s\n", id, printable(ref.apiType, 40),
			printable(notice.ContentType, 40), len(notice.Body), errText(err))
		return nil
	}

	hosts := urlHosts(notice.Body)
	for _, h := range hosts {
		stats.platforms[h]++
	}
	stats.flavours[valueOrNone(string(header.Flavour), 20)]++
	stats.roots[valueOrNone(header.Root, 40)]++
	stats.sdks[valueOrNone(header.SDKVersion, 12)]++
	stats.subtypes[valueOrNone(header.SubTypeCode, 12)]++

	p.printf("  %s | %s | %s | sdk %s | %s | %s/%s | v%s | %d bytes | %s\n",
		id, printable(ref.apiType, 40), printable(header.Root, 30), printable(header.SDKVersion, 12),
		valueOrNone(string(header.Flavour), 20), printable(header.NoticeTypeCode, 20), printable(header.SubTypeCode, 12),
		printable(header.VersionID, 8), len(notice.Body), strings.Join(firstN(hosts, 3), ","))
	return nil
}

// valueOrNone is printable(s, max), with "none" for an absent value so that a
// missing field is visible in a count instead of an empty name.
func valueOrNone(s string, max int) string {
	if v := printable(s, max); v != "" {
		return v
	}
	return "none"
}

// printCounts prints "name: a=3 b=1", largest first, and at most limit entries.
func (p *probe) printCounts(name string, counts map[string]int, limit int) {
	type kv struct {
		key string
		n   int
	}
	rows := make([]kv, 0, len(counts))
	for k, n := range counts {
		rows = append(rows, kv{k, n})
	}
	sort.Slice(rows, func(i, j int) bool {
		if rows[i].n != rows[j].n {
			return rows[i].n > rows[j].n
		}
		return rows[i].key < rows[j].key
	})
	shown := rows
	if len(shown) > limit {
		shown = shown[:limit]
	}
	parts := make([]string, 0, len(shown)+1)
	for _, r := range shown {
		parts = append(parts, fmt.Sprintf("%s=%d", r.key, r.n))
	}
	if more := len(rows) - len(shown); more > 0 {
		parts = append(parts, fmt.Sprintf("and %d more", more))
	}
	p.printf("  %s: %s\n", name, strings.Join(parts, " "))
}

// fetch runs a search and saves the raw body whether or not it decoded.
func (p *probe) fetch(ctx context.Context, label string, params doffin.SearchParams) (doffin.SearchResponse, error) {
	resp, err := p.client.Search(ctx, params)
	if len(resp.Raw) > 0 {
		p.save("search-"+label+".json", resp.Raw)
	}
	return resp, err
}

// search runs a search and prints a summary and the first hits.
func (p *probe) search(ctx context.Context, label string, params doffin.SearchParams) (doffin.SearchResponse, error) {
	resp, err := p.fetch(ctx, label, params)
	if err != nil {
		p.printf("    failed: %s\n", errText(err))
		return resp, err
	}
	p.printf("    numHitsTotal=%d numHitsAccessible=%d hits=%d\n", resp.NumHitsTotal, resp.NumHitsAccessible, len(resp.Hits))
	for i, hit := range resp.Hits {
		if i == 3 {
			break
		}
		p.printf("    %s\n", describe(hit))
	}
	return resp, nil
}

func (p *probe) save(name string, data []byte) {
	if p.saveDir == "" {
		return
	}
	path, err := writeFileAtomic(p.saveDir, name, data)
	if err != nil {
		p.printf("    (could not save %s: %s)\n", name, errText(err))
		return
	}
	p.printf("    saved %s (%d bytes)\n", path, len(data))
}

// writeFileAtomic writes data to dir/name with owner-only permissions. It writes
// a private temporary file and renames it into place, so a symlink planted at
// the destination is replaced rather than written through, an existing capture
// is never left half-written, and the mode does not depend on what was there.
func writeFileAtomic(dir, name string, data []byte) (string, error) {
	if name == "" || name == "." || name == ".." || name != filepath.Base(name) || strings.HasPrefix(name, ".") || strings.ContainsAny(name, `/\`) {
		return "", fmt.Errorf("unsafe file name %q", name)
	}
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return "", err
	}
	tmp, err := os.CreateTemp(dir, ".probe-*")
	if err != nil {
		return "", err
	}
	tmpName := tmp.Name()
	if _, err := tmp.Write(data); err != nil {
		_ = tmp.Close()
		_ = os.Remove(tmpName)
		return "", err
	}
	if err := tmp.Close(); err != nil {
		_ = os.Remove(tmpName)
		return "", err
	}
	target := filepath.Join(dir, name)
	if err := os.Rename(tmpName, target); err != nil {
		_ = os.Remove(tmpName)
		return "", err
	}
	return target, nil
}

// describe renders one hit on a line. Every string that came from the API goes
// through printable.
func describe(h doffin.Hit) string {
	buyer := "-"
	if len(h.Buyer) > 0 {
		buyer = printable(h.Buyer[0].Name, 36)
	}
	return fmt.Sprintf("%s | %s | status=%s | published %s | deadline=%s | buyer=%s | lots=%d cpv=%d | %s",
		printable(h.ID, 24), printable(h.Type, 40), orDash(h.Status), printable(h.PublicationDate, 32),
		orDash(h.Deadline), buyer, len(h.Lots), len(h.CPVCodes), printable(h.Heading, 60))
}

func orDash(s *string) string {
	if s == nil {
		return "-"
	}
	return printable(*s, 40)
}

// refsOf lists the notices of a search result. When the typed decode produced no
// hits but the raw body has some, the ids come from the raw body, so a shape the
// spec gets wrong still leads to downloads; fromRaw says so.
func refsOf(resp doffin.SearchResponse) (refs []sampleRef, fromRaw bool) {
	refs = make([]sampleRef, 0, len(resp.Hits))
	for _, h := range resp.Hits {
		refs = append(refs, sampleRef{id: h.ID, apiType: h.Type})
	}
	if len(refs) > 0 {
		return refs, false
	}
	for _, id := range idsFromRaw(resp.Raw) {
		refs = append(refs, sampleRef{id: id, apiType: "?"})
	}
	return refs, len(refs) > 0
}

// capRefs keeps the first n notices, so a response carrying more hits than were
// asked for cannot widen the sample.
func capRefs(refs []sampleRef, n int) []sampleRef {
	if n > 0 && len(refs) > n {
		return refs[:n]
	}
	return refs
}

func firstID(resp doffin.SearchResponse) string {
	if refs, _ := refsOf(resp); len(refs) > 0 {
		return refs[0].id
	}
	return ""
}

// idsFromRaw reads every hit id from a raw search body, whether hits is a list
// or a single object.
func idsFromRaw(raw []byte) []string {
	var envelope struct {
		Hits json.RawMessage `json:"hits"`
	}
	if len(raw) == 0 || json.Unmarshal(raw, &envelope) != nil {
		return nil
	}
	var list []struct {
		ID string `json:"id"`
	}
	if err := json.Unmarshal(envelope.Hits, &list); err == nil {
		ids := make([]string, 0, len(list))
		for _, h := range list {
			if h.ID != "" {
				ids = append(ids, h.ID)
			}
		}
		return ids
	}
	var one struct {
		ID string `json:"id"`
	}
	if err := json.Unmarshal(envelope.Hits, &one); err == nil && one.ID != "" {
		return []string{one.ID}
	}
	return nil
}

var (
	urlHostPattern = regexp.MustCompile(`https?://([A-Za-z0-9][A-Za-z0-9.-]*[A-Za-z0-9])`)
	// namespaceHosts appear in xmlns declarations and say nothing about where a
	// tender is hosted.
	namespaceHosts = map[string]bool{"www.w3.org": true, "data.europa.eu": true, "docs.oasis-open.org": true}
)

// urlHosts lists the distinct hosts of the URLs in a document, which shows
// which tender platforms buyers use. Namespace URIs are ignored, and at most
// maxHostsPerNotice are returned. It scans forward rather than collecting every
// match first, so a document full of URLs costs no more than it needs to.
func urlHosts(body []byte) []string {
	seen := map[string]bool{}
	var hosts []string
	for rest := body; len(hosts) < maxHostsPerNotice; {
		loc := urlHostPattern.FindSubmatchIndex(rest)
		if loc == nil {
			break
		}
		host := strings.ToLower(string(rest[loc[2]:loc[3]]))
		rest = rest[loc[1]:]
		if namespaceHosts[host] || seen[host] {
			continue
		}
		seen[host] = true
		hosts = append(hosts, printable(host, 60))
	}
	sort.Strings(hosts)
	return hosts
}

func firstN(items []string, n int) []string {
	if len(items) > n {
		return items[:n]
	}
	return items
}

func errText(err error) string { return printable(err.Error(), 300) }

// extensionFor picks a file extension from the body, because the API labels
// every download application/octet-stream whatever it holds.
func extensionFor(contentType string, body []byte) string {
	trimmed := bytes.TrimLeft(bytes.TrimPrefix(body, []byte{0xEF, 0xBB, 0xBF}), " \t\r\n")
	lower := bytes.ToLower(trimmed[:min(len(trimmed), 16)])
	switch {
	case bytes.HasPrefix(lower, []byte("%pdf")):
		return ".pdf"
	case bytes.HasPrefix(trimmed, []byte("PK\x03\x04")):
		return ".zip"
	case bytes.HasPrefix(lower, []byte("<html")), bytes.HasPrefix(lower, []byte("<!doctype html")):
		return ".html"
	case bytes.HasPrefix(trimmed, []byte("<")):
		return ".xml"
	case bytes.HasPrefix(trimmed, []byte("{")), bytes.HasPrefix(trimmed, []byte("[")):
		return ".json"
	}
	switch ct := strings.ToLower(contentType); {
	case strings.Contains(ct, "xml"):
		return ".xml"
	case strings.Contains(ct, "json"):
		return ".json"
	case strings.Contains(ct, "html"):
		return ".html"
	case strings.Contains(ct, "pdf"):
		return ".pdf"
	case strings.Contains(ct, "zip"):
		return ".zip"
	}
	return ".bin"
}

// printable makes upstream text safe to show: control characters (ESC, BEL,
// newlines) become spaces, invisible format characters (bidi overrides,
// zero-width spaces) are dropped, runs of whitespace collapse, and the result
// is cut to max runes.
func printable(s string, max int) string {
	s = strings.Map(func(r rune) rune {
		switch {
		case unicode.IsControl(r):
			return ' '
		case unicode.Is(unicode.Cf, r):
			return -1
		}
		return r
	}, s)
	s = strings.Join(strings.Fields(s), " ")
	if runes := []rune(s); len(runes) > max {
		return string(runes[:max]) + "…"
	}
	return s
}
