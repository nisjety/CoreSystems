package driver

import (
	"context"

	"github.com/go-rod/rod"
)

// PageProvider is satisfied by any type that can supply a pre-warmed browser
// page and a release function. The caller MUST invoke release() when done with
// the page so the page can be reset and returned to the pool.
//
// *scraper.BrowserPool satisfies this interface automatically (structural
// typing), which breaks the import cycle: driver imports no scraper types.
type PageProvider interface {
	GetPage(ctx context.Context) (*rod.Page, func(), error)
}
