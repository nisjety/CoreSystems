package watch

import "time"

// ObserveGrant is what one fresh Control observe decision lets a watch record (D18).
//
// Control signs the decision with the member's CURRENT history window, so a window that moved after the watch
// was created is the one applied: the watch row keeps no window of its own, because a stored window is exactly
// what goes stale. The window is a presentation narrowing of a disclosure that is already authorised. It can
// only HIDE, and it is honoured with no rollout switch, because a hide must never depend on one.
//
// A watch records content a program wrote into a room the member is a recipient of. A member whose window
// starts at an instant must not be told what the program printed before it, however the watch came to be.
type ObserveGrant struct {
	// HistoryFloorAt is the instant the member's window starts at (a since_date window, or a member's join).
	// Nil means no timestamp window.
	HistoryFloorAt *time.Time
	// HistoryFloorRevision is the first recipient-audience revision a WS04-S11 since_join window starts at.
	// Zero means no revision window.
	HistoryFloorRevision int64
}

// Windowed reports whether the grant narrows what may be recorded at all.
func (g ObserveGrant) Windowed() bool {
	return g.HistoryFloorAt != nil || g.HistoryFloorRevision > 0
}

// admitsSource reports whether the watched source as a whole lies inside the revision window. The source
// carries the audience revision it was registered under, and a source registered before the window's
// revision belongs to the room's past. An unstamped source (revision zero) cannot be placed, so a revision
// window hides it: it fails closed, where a ceiling on the same stamp does not.
func (g ObserveGrant) admitsSource(sourceRevision int64) bool {
	return g.HistoryFloorRevision <= 0 || sourceRevision >= g.HistoryFloorRevision
}

// admitsAt reports whether something the source did at `at` lies inside the timestamp window. An instant the
// source did not report (the zero time) cannot be placed, so a timestamp window hides it.
func (g ObserveGrant) admitsAt(at time.Time) bool {
	if g.HistoryFloorAt == nil {
		return true
	}
	return !at.IsZero() && !at.Before(*g.HistoryFloorAt)
}

// narrow returns the poll as the window shows it: the lines captured inside it, and no more.
//
//   - A line is shown only when its source captured it at or after the window. A line the source gave no
//     instant for is hidden.
//   - A gap before the first line says output was trimmed somewhere between the cursor and here. Under a
//     timestamp window that output cannot be placed in time, so the notice is withheld; a revision window
//     over an admitted source keeps it, because the whole source is inside the window.
//   - The source's end is told only when it ended inside the window. The watch still ends with its source:
//     what is withheld is the event, not the state.
//   - A source registered before a revision window shows nothing at all.
//
// The cursor and the terminal state are left as the poll found them, so the watch moves past what it hid
// instead of meeting it again on the next poll.
func (g ObserveGrant) narrow(result PollResult) PollResult {
	narrowed := result
	narrowed.Lines = nil
	if g.HistoryFloorAt != nil {
		narrowed.GapBefore = false
	}
	if !g.admitsSource(result.SourceRevision) {
		narrowed.GapBefore = false
		narrowed.terminalHidden = result.SourceTerminal
		return narrowed
	}
	for _, line := range result.Lines {
		if g.admitsAt(line.CapturedAt) {
			narrowed.Lines = append(narrowed.Lines, line)
		}
	}
	if result.SourceTerminal && !g.admitsAt(result.EndedAt) {
		narrowed.terminalHidden = true
	}
	return narrowed
}
