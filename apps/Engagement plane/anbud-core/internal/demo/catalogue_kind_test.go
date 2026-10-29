package demo

import "testing"

func TestEveryHeldNoticeTypeHasAKind(t *testing.T) {
	for noticeType, want := range map[string]string{
		"cn-standard": "competition", "cn-social": "competition",
		"pin-only": "planning", "pin-rtl": "planning", "pin-buyer": "planning", "pin-cfc-social": "planning", "pmc": "planning",
		"can-standard": "result", "can-social": "result", "can-modif": "result", "can-tran": "result", "veat": "result",
		"": "other",
	} {
		if got := catalogueKind(CatalogueNotice{Result: Result{NoticeType: noticeType}}); got != want {
			t.Errorf("%q: kind %q, want %q", noticeType, got, want)
		}
	}
}
