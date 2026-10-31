package siteaudit

import "sort"

// Legacy stores contain only a bounded selection of full reports. Backfill
// exactly those observations, clearly marking the unknown pre-upgrade history.
func bootstrapInsights(state diskState) (diskState, error) {
	state.InsightVersion, state.Insights = 1, []insightRecord{}
	runs := append([]Snapshot{}, state.Runs...)
	sort.SliceStable(runs, func(i, j int) bool { return mustTime(runs[i].StartedAt).Before(mustTime(runs[j].StartedAt)) })
	for _, run := range runs {
		var err error
		state, err = recordInsights(state, run)
		if err != nil {
			return diskState{}, err
		}
	}
	for i, value := range state.Insights {
		value.HistoryLimited = true
		state.Insights[i] = value
	}
	return state, nil
}
