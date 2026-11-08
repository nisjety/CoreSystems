package store_test

import (
	"testing"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store/storetest"
)

func TestMemoryJobAdmission(t *testing.T) {
	t.Parallel()
	storetest.AdmissionConformance(t, func(t *testing.T) store.DB { return store.NewMemory() })
}

func TestJobAdmissionCap(t *testing.T) {
	t.Parallel()
	for name, tc := range map[string]struct {
		params map[string]any
		want   int
	}{
		"none":        {nil, 7},
		"float":       {map[string]any{"admission": map[string]any{"cap": float64(2)}}, 2},
		"int":         {map[string]any{"admission": map[string]any{"cap": 4}}, 4},
		"zero":        {map[string]any{"admission": map[string]any{"cap": float64(0)}}, 7},
		"negative":    {map[string]any{"admission": map[string]any{"cap": float64(-3)}}, 7},
		"huge":        {map[string]any{"admission": map[string]any{"cap": float64(1e12)}}, 7},
		"string":      {map[string]any{"admission": map[string]any{"cap": "2"}}, 7},
		"not a map":   {map[string]any{"admission": "yes"}, 7},
		"missing cap": {map[string]any{"admission": map[string]any{}}, 7},
	} {
		if got := store.JobAdmissionCap(store.Job{Params: tc.params}, 7); got != tc.want {
			t.Errorf("%s: cap = %d, want %d", name, got, tc.want)
		}
	}
}
