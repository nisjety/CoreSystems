package handler

import (
	"context"
	"net/http"
)

type contextKey string

const orgIDKey contextKey = "org_id"

func OrgIDMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		orgID := r.Header.Get("X-Org-ID")
		if orgID == "" {
			http.Error(w, `{"error":"X-Org-ID header required"}`, http.StatusBadRequest)
			return
		}
		ctx := context.WithValue(r.Context(), orgIDKey, orgID)
		next.ServeHTTP(w, r.WithContext(ctx))
	})
}

func OrgIDFrom(ctx context.Context) string {
	v, _ := ctx.Value(orgIDKey).(string)
	return v
}
