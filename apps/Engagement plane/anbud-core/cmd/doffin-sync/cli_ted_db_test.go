package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/db/dbtest"
	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/eforms/eformstest"
	"github.com/triodelab/anbud/internal/rawstore"
)

// Needs a throwaway server on ANBUD_TEST_DSN: run it with make test-db. The TED source end to end,
// as the least-privileged role: the command searches a loopback TED for one publication day,
// downloads the notices it does not hold, stores them under source ted, and a reader finds each under
// its public id and parses it. No Doffin key is set: TED needs none.
func TestCLISyncsTEDAsTheApplicationRole(t *testing.T) {
	rs := dbtest.RoleDatabase(t)
	if err := db.ApplyMigrations(context.Background(), dbtest.Pool(t, rs.MigratorDSN)); err != nil {
		t.Fatalf("migrate: %s", dbtest.Describe(err))
	}
	day := loopbackDay()
	numbers := []string{"700001-" + day.Format("2006"), "700002-" + day.Format("2006")}

	var (
		mu        sync.Mutex
		queries   []string
		downloads []string
	)
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		defer mu.Unlock()
		switch {
		case r.Method == http.MethodPost && r.URL.Path == "/search":
			var req struct {
				Query string `json:"query"`
			}
			body, _ := io.ReadAll(r.Body)
			_ = json.Unmarshal(body, &req)
			queries = append(queries, req.Query)
			w.Header().Set("Content-Type", "application/json")
			_, _ = fmt.Fprintf(w, `{"notices":[{"publication-number":%q},{"publication-number":%q}],"totalNoticeCount":2}`, numbers[0], numbers[1])
		case strings.HasPrefix(r.URL.Path, "/en/notice/"):
			id := strings.TrimSuffix(strings.TrimPrefix(r.URL.Path, "/en/notice/"), "/xml")
			downloads = append(downloads, id)
			w.Header().Set("Content-Type", "application/xml")
			_, _ = w.Write(eformstest.ContractNotice(eformstest.Notice{
				ID: fmt.Sprintf("00000000-0000-4000-8000-%012s", strings.ReplaceAll(id, "-", "")), CustomizationID: "eforms-sdk-1.13", SDK: "1.13", Title: "Ramavtal " + id,
			}))
		default:
			w.WriteHeader(http.StatusNotFound)
		}
	}))
	t.Cleanup(srv.Close)

	env := map[string]string{
		"ANBUD_DSN":         rs.AppDSN,
		"ANBUD_MIGRATE_DSN": rs.MigratorDSN,
		"TED_SEARCH_URL":    srv.URL + "/search",
		"TED_NOTICE_URL":    srv.URL,
	}
	dbtest.MustBeThrowaway(t, env["ANBUD_DSN"], env["ANBUD_MIGRATE_DSN"])
	var out, errOut bytes.Buffer
	code := cli([]string{"-source", "ted", "-countries", "SWE,DNK", "-from", day.Format(dateLayout), "-to", day.Format(dateLayout)}, mapEnv(env), &out, &errOut)
	if code != exitDone {
		t.Fatalf("exit %d, stderr %q, stdout %q", code, errOut.String(), out.String())
	}
	assertNoDSNParts(t, out.String()+errOut.String(), rs.AppDSN, rs.MigratorDSN)

	mu.Lock()
	if len(queries) == 0 || queries[0] != "publication-date="+day.Format("20060102")+" AND buyer-country IN (SWE DNK)" {
		t.Errorf("queries %v", queries)
	}
	if len(downloads) != 2 {
		t.Errorf("downloads %v, want each notice once", downloads)
	}
	mu.Unlock()

	store := rawstore.New(dbtest.Pool(t, rs.AppDSN))
	for _, n := range numbers {
		doc, err := store.Notice(context.Background(), "TED-"+n)
		if err != nil || doc.Source != "ted" {
			t.Fatalf("Notice(TED-%s) = %+v, %v", n, doc, err)
		}
		parsed, err := eforms.Parse(doc.Body)
		if err != nil || parsed.Title != "Ramavtal "+n {
			t.Errorf("TED-%s parses as %q, %v", n, parsed.Title, err)
		}
	}

	// A second run of the same day calls nothing it holds: the day is done.
	out.Reset()
	errOut.Reset()
	if code := cli([]string{"-source", "ted", "-countries", "SWE,DNK", "-from", day.Format(dateLayout), "-to", day.Format(dateLayout)}, mapEnv(env), &out, &errOut); code != exitDone {
		t.Fatalf("second run: exit %d, stderr %q", code, errOut.String())
	}
	mu.Lock()
	defer mu.Unlock()
	if len(downloads) != 2 {
		t.Errorf("the second run downloaded again: %v", downloads)
	}
}
