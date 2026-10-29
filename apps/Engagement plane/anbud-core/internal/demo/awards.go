package demo

import (
	"context"
	"fmt"

	"github.com/triodelab/anbud/internal/incumbency"
	"github.com/triodelab/anbud/internal/rawstore"
)

// AllNoticeSource is a notice source that can read every stored body, not only the newest. The raw store
// is one.
type AllNoticeSource interface {
	AllNotices(ctx context.Context, fn func(rawstore.Document) error) error
}

// awardsTimeout bounds a read of the whole store. It is longer than a scan of the newest notices, since
// a suggestion reads all of them.
const awardsTimeout = 2 * defaultScanTimeout

// Awards returns the awarded lots the notice store holds. A company's tender profile is drawn from the
// ones it won, so this reads every stored notice when the store can, and the newest MaxScan otherwise.
func (s *Service) Awards(ctx context.Context) ([]incumbency.Award, error) {
	scanCtx, cancel := context.WithTimeout(ctx, awardsTimeout)
	defer cancel()
	each := func(fn func(rawstore.Document) error) error {
		if all, ok := s.notices.(AllNoticeSource); ok {
			return all.AllNotices(scanCtx, fn)
		}
		return s.notices.RecentNotices(scanCtx, MaxScan, fn)
	}

	type scanned struct {
		c    candidate
		kind readKind
	}
	var reads []scanned
	var copies []twin
	err := each(func(doc rawstore.Document) error {
		if err := scanCtx.Err(); err != nil {
			return err
		}
		c, kind, _ := read(doc)
		reads = append(reads, scanned{c: c, kind: kind})
		copies = append(copies, twin{noticeID: c.Notice.NoticeID, version: c.Notice.VersionID, source: doc.Source})
		return nil
	})
	if err != nil {
		return nil, fmt.Errorf("demo: read the awards: %w", err)
	}
	keep := keepOnePerNotice(copies)
	var awards []incumbency.Award
	for i, r := range reads {
		if keep[i] && r.kind == kindAward {
			awards = append(awards, incumbency.FromNotice(r.c.id, r.c.Notice)...)
		}
	}
	return awards, nil
}
