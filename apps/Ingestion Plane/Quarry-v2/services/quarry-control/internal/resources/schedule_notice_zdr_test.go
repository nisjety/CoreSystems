package resources

import (
	"context"
	"encoding/json"
	"net/http"
	"testing"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/notify"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// zdrFlagCapture is a notify.Sink that keeps the zdr flag of each schedule notice.
type zdrFlagCapture struct {
	noticeTypes []string
	zdr         []bool
}

func (s *zdrFlagCapture) NotifyChange(context.Context, string, string, string, map[string]any) error {
	return nil
}

func (s *zdrFlagCapture) NotifyScheduleRun(_ context.Context, _, _, _, noticeType string, zdr bool, _ map[string]string) error {
	s.noticeTypes = append(s.noticeTypes, noticeType)
	s.zdr = append(s.zdr, zdr)
	return nil
}

// seedStampedSchedule stores an enabled batch schedule of org_a, created by user_1,
// with the given create-time ZDR stamp. A nil stamp is a legacy row.
func seedStampedSchedule(t *testing.T, db store.DB, name string, stamp *bool) store.Schedule {
	t.Helper()
	s := store.Schedule{
		ID:            quarrycontracts.NewID(quarrycontracts.KindSchedule),
		OrgID:         "org_a",
		Cron:          "* * * * *",
		TargetKind:    "batch",
		TargetRef:     "https://example.com",
		Enabled:       true,
		CreatedAt:     1,
		CreatedBy:     "user_1",
		Name:          name,
		Config:        map[string]any{"url": "https://example.com", "max_pages": 5},
		Timezone:      "UTC",
		OverlapPolicy: "skip",
		UpdatedAt:     1,
		Zdr:           stamp,
	}
	if err := db.Schedules().Create(s); err != nil {
		t.Fatalf("seed schedule %s: %v", name, err)
	}
	return s
}

// A skipped_zdr fire is denied because the org's effective posture is ZDR now, so its
// notice is ZDR whatever the create-time stamp says: an org that turns ZDR on keeps the
// stamp of the schedules it already had. Every other skip code keeps the stamp's retention.
func TestSkippedFireNoticeZdrFollowsStampOrSkippedZdr(t *testing.T) {
	t.Parallel()
	db := store.NewMemory()
	sink := &zdrFlagCapture{}
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountSchedules(r, db, WithScheduleNotices(sink))

	yes, no := true, false
	cases := []struct {
		name   string
		stamp  *bool
		reason string
		want   bool
	}{
		{"legacy_unstamped_skipped_zdr", nil, "skipped_zdr", true},
		{"stamped_standard_skipped_zdr", &no, "skipped_zdr", true},
		{"legacy_unstamped_skipped_overlap", nil, "skipped_overlap", false},
		{"stamped_zdr_skipped_overlap", &yes, "skipped_overlap", true},
	}
	for i, tc := range cases {
		s := seedStampedSchedule(t, db, tc.name, tc.stamp)
		body, err := json.Marshal(map[string]string{
			"run_key": tc.name,
			"run_id":  "run_" + tc.name,
			"status":  "skipped",
			"reason":  tc.reason,
		})
		if err != nil {
			t.Fatalf("encode body: %v", err)
		}
		if w := postScheduleRun(r, s.ID, string(body)); w.Code != http.StatusCreated {
			t.Fatalf("%s: registration = %d; body=%s", tc.name, w.Code, w.Body.String())
		}
		if len(sink.zdr) != i+1 {
			t.Fatalf("%s: %d schedule notices sent, want %d", tc.name, len(sink.zdr), i+1)
		}
		if sink.noticeTypes[i] != notify.ScheduleRunSkippedType {
			t.Fatalf("%s: notice type = %q, want %q", tc.name, sink.noticeTypes[i], notify.ScheduleRunSkippedType)
		}
		if got := sink.zdr[i]; got != tc.want {
			t.Errorf("%s: notice zdr = %v, want %v", tc.name, got, tc.want)
		}
	}
}
