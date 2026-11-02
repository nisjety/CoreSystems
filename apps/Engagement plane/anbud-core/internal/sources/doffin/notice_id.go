package doffin

// IsNoticeID reports whether id has the shape of a Doffin notice id, such as
// 2025-104837. It applies the same pattern Download checks before any call, so a
// store can refuse exactly the ids the client refuses.
func IsNoticeID(id string) bool {
	return noticeIDPattern.MatchString(id)
}
