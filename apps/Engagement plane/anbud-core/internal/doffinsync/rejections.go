package doffinsync

// The bounded retry of notices whose download was refused for its content: the body
// was too large or empty (R1). The store reports facts in a Resolution; this file is
// the policy, applied with the engine's own clock (Options.Now), so the stored
// instants and the clock that judges them are one clock domain.
//
//   - Fewer than MaxRejections refusals, the latest under RejectCooloff old: the id
//     is cooling. It is unresolved with item_rejected and it is not downloaded. It
//     makes no attempt row and does not touch the consecutive-error count.
//   - Fewer than MaxRejections refusals, the latest RejectCooloff old or older: an
//     ordinary download candidate.
//   - MaxRejections refusals or more: terminal. The id is never downloaded again. It
//     counts as rejected in its own bucket, like a terminal 404, so the day can be done.
//   - A download refused now that brings the total to MaxRejections is terminal in
//     the same run: it counts as rejected for the day, not as unresolved.

// candidate is an id the engine will download, with the content refusals the store
// already holds for it.
type candidate struct {
	id       string
	refusals int
}

// refusalsOf is the number of content refusals the store reports for an id. Only a
// rejected resolution carries a count (R3).
func refusalsOf(rs Resolution) int {
	if rs.Kind == ResolutionRejected {
		return rs.Rejections
	}
	return 0
}

// cooling reports whether the latest refusal of a rejected id is too recent to ask
// Doffin again: younger than RejectCooloff by the engine's clock. A refusal stamped
// in the future is cooling too.
func (r *run) cooling(rs Resolution) bool {
	return r.e.now().Sub(rs.LastRejection) < RejectCooloff
}

// refused counts a download that was refused for its content just now. It is the
// id's refusals+1-th. At MaxRejections the id is terminal and counts as rejected;
// before that it stays unresolved until the cool-off has passed. A refusal neither
// counts toward the five in a row nor resets it (D-B1).
func (r *run) refused(c candidate, rec *DayRecord) {
	if c.refusals+1 >= MaxRejections {
		rec.Rejected++
		return
	}
	markUnresolved(rec, CodeItemRejected)
}
