// Command link-probe measures whether the tender document links that captured notices publish are
// open to anyone. It reads the notices in a directory, asks each distinct link once with no cookie
// and no login, and prints counts only: never a link, a host, a notice or a byte of a document. The
// known login platforms are counted but not requested at all. With -hop-max, each HTML page the
// first hop found is read once, and the document links it holds on its own host are asked once each.
package main

import (
	"context"
	"flag"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"slices"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/linkprobe"
)

func main() {
	os.Exit(run(os.Args[1:], os.Stdout, os.Stderr, linkprobe.NewClient(), time.Sleep))
}

// run does the whole probe and returns the exit code. client and pause are passed in so a test
// can answer from a local server and need not wait.
func run(args []string, stdout, stderr io.Writer, client *http.Client, pause func(time.Duration)) int {
	fs := flag.NewFlagSet("link-probe", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	dir := fs.String("dir", "testdata/captured", "directory of captured notices (*.xml)")
	maxLinks := fs.Int("max", 60, "most links to request")
	gap := fs.Duration("gap", 2*time.Second, "pause between two requests")
	hopMax := fs.Int("hop-max", 0, "document links to request on each HTML page the first hop found (0: no second hop)")
	if err := fs.Parse(args); err != nil || fs.NArg() > 0 || *maxLinks < 0 || *hopMax < 0 {
		fmt.Fprintln(stderr, "usage: link-probe [-dir DIR] [-max N] [-gap D] [-hop-max N]")
		return 2
	}

	files, err := filepath.Glob(filepath.Join(*dir, "*.xml"))
	if err != nil || len(files) == 0 {
		fmt.Fprintln(stderr, "link-probe: no captured notices in the directory")
		return 1
	}

	rep := newReport()
	links := map[string]bool{}
	for _, file := range files {
		data, err := os.ReadFile(file)
		if err != nil {
			rep.refused++
			continue
		}
		found, err := eforms.ExternalLinks(data)
		if err != nil {
			rep.refused++
			continue
		}
		rep.notices++
		if len(found) > 0 {
			rep.withLinks++
		}
		for _, l := range found {
			links[l] = true
		}
	}

	ordered := make([]string, 0, len(links))
	for l := range links {
		ordered = append(ordered, l)
	}
	slices.Sort(ordered)
	rep.distinct = len(ordered)

	var toRequest []string
	for _, l := range ordered {
		fam := linkprobe.Family(l)
		rep.families[fam]++
		if fam == "login-platform" {
			rep.skipped++
			continue
		}
		toRequest = append(toRequest, l)
	}
	if len(toRequest) > *maxLinks {
		toRequest = toRequest[:*maxLinks]
		rep.capped = true
	}

	ctx := context.Background()
	var pages []string
	for i, l := range toRequest {
		if i > 0 {
			pause(*gap)
		}
		class := linkprobe.Classify(linkprobe.Fetch(ctx, client, l))
		fam := linkprobe.Family(l)
		rep.requested++
		rep.results[class.String()]++
		if rep.byFamily[fam] == nil {
			rep.byFamily[fam] = map[string]int{}
		}
		rep.byFamily[fam][class.String()]++
		if class == linkprobe.HTMLPage {
			pages = append(pages, l)
		}
	}

	if *hopMax > 0 {
		hop := newHop()
		for _, page := range pages {
			pause(*gap)
			body, final, err := linkprobe.FetchPage(ctx, client, page)
			if err != nil {
				hop.unreadable++
				continue
			}
			hop.pages++
			same, other := linkprobe.DocumentLinks(body, final)
			hop.found += len(same)
			hop.otherHost += other
			if len(same) > *hopMax {
				same = same[:*hopMax]
				hop.capped = true
			}
			for _, l := range same {
				pause(*gap)
				class := linkprobe.Classify(linkprobe.Fetch(ctx, client, l))
				hop.requested++
				hop.results[class.String()]++
			}
		}
		rep.hop = &hop
	}

	writeReport(stdout, rep)
	return 0
}
