package algoliamcp

import (
	"encoding/json"
	"sort"

	"github.com/triodelab/integration-corev2/internal/webvitals"
)

// This exact applications/applicationId envelope was verified on the hosted
// provider. Only IDs cross Integration's boundary; all account metadata stays out.
func parseHostedApplications(raw json.RawMessage) ([]Application, error) {
	if len(raw) > maxResponse || webvitals.UniqueJSON(raw) != nil {
		return nil, ErrUnavailable
	}
	root := shapeObject(raw)
	if len(root) != 1 || !present(root, "applications") {
		return nil, ErrUnavailable
	}
	var rows []json.RawMessage
	if json.Unmarshal(root["applications"], &rows) != nil || rows == nil || len(rows) > 128 {
		return nil, ErrUnavailable
	}
	items := make([]Application, 0, len(rows))
	seen := map[string]bool{}
	for _, row := range rows {
		fields := shapeObject(row)
		for _, key := range []string{"id", "applicationID", "application_id", "error", "errors", "isError", "nextCursor", "hasMore"} {
			if present(fields, key) {
				return nil, ErrUnavailable
			}
		}
		var id string
		if json.Unmarshal(fields["applicationId"], &id) != nil || !appPattern.MatchString(id) || seen[id] {
			return nil, ErrUnavailable
		}
		seen[id] = true
		items = append(items, Application{ID: id})
	}
	sort.Slice(items, func(i, j int) bool { return items[i].ID < items[j].ID })
	return items, nil
}
