package watch

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"strings"
	"time"
)

// DefaultTTL and MaxTTL bound how long a watch may stand.
//
// No unbounded watches: a standing intent with no end is a standing cost, and
// the person who set it has long stopped expecting it. The default is a working
// day because that is the span of "I'm waiting on this build"; the ceiling is a
// week because anything longer is a monitor, which is S4.4's problem and not a
// watch's.
const (
	DefaultTTL = 8 * time.Hour
	MaxTTL     = 7 * 24 * time.Hour
)

// MaxActiveWatchesPerSpace is the concurrency bound the design states. The same
// count-bounded shape S4.2 gave processes themselves.
const MaxActiveWatchesPerSpace = 8

// CreateAuthorizer obtains Control's `model.watch.create` decision for one
// requested watch.
//
// Separate from [Reauthorizer] on purpose, and not merely because the actions
// differ: this one runs with a HUMAN present, so Control resolves the subject
// from the verified delegation rather than being told who it is.
type CreateAuthorizer interface {
	AuthorizeCreate(ctx context.Context, w Watch) (Authority, error)
}

// SourceResolver proves that a requested `source_ref` is a real resource in the
// requesting Space, and reports where a watch on it should start reading.
//
// # Why creation needs this at all
//
// The adapter already refuses a foreign source at poll time, so nothing leaks
// without it. But a watch created against another Space's process would sit
// ACTIVE, consume a slot, and fail silently forever — the person would be told
// their watch exists and would never hear from it again. Refusing at create is
// the difference between an error and a lie.
//
// The starting cursor comes from here rather than defaulting to zero because a
// watch created on a process that has been running for an hour should not
// replay that hour: the person asked to be told what happens NEXT.
type SourceResolver interface {
	Resolve(ctx context.Context, w Watch) (startCursor int64, err error)
}

// Handler is capability-core's HTTP surface for watches.
type Handler struct {
	store     *Store
	authz     CreateAuthorizer
	resolvers map[string]SourceResolver
	newID     func() string
	nowFn     func() time.Time
}

// NewHandler builds the surface. A kind with no resolver cannot be watched:
// creation refuses rather than accepting a watch nothing can serve.
func NewHandler(store *Store, authz CreateAuthorizer, resolvers map[string]SourceResolver, newID func() string) (*Handler, error) {
	if store == nil || authz == nil || newID == nil {
		return nil, fmt.Errorf("a watch handler requires a store, a create authorizer and an id source")
	}
	return &Handler{
		store:     store,
		authz:     authz,
		resolvers: resolvers,
		newID:     newID,
		nowFn:     func() time.Time { return time.Now().UTC() },
	}, nil
}

type createRequest struct {
	SpaceRef   string `json:"space_ref"`
	SourceKind string `json:"source_kind"`
	SourceRef  string `json:"source_ref"`
	Predicate  struct {
		Kind   string `json:"kind"`
		Value  string `json:"value"`
		Stream string `json:"stream"`
	} `json:"predicate"`
	TTLSeconds int64 `json:"ttl_seconds"`
}

type watchView struct {
	ID          string  `json:"id"`
	SpaceRef    string  `json:"space_ref"`
	SourceKind  string  `json:"source_kind"`
	SourceRef   string  `json:"source_ref"`
	Predicate   string  `json:"predicate"`
	State       string  `json:"state"`
	ExpiresAt   string  `json:"expires_at"`
	LastEventAt *string `json:"last_event_at"`
	// LastEvent is a REDACTED, bounded summary — the source scrubbed it and the
	// column caps it. It is still content a program chose, which is why the
	// event log labels its trust and this view is read-only.
	LastEvent string `json:"last_event"`
}

func view(w *Watch) watchView {
	predicate := w.Predicate.Kind
	if w.Predicate.Value != "" {
		predicate += " " + w.Predicate.Value
	}
	if w.Predicate.Stream != "" {
		predicate += " on " + w.Predicate.Stream
	}
	v := watchView{
		ID: w.ID, SpaceRef: w.SpaceRef,
		SourceKind: w.SourceKind, SourceRef: w.SourceRef,
		Predicate: predicate, State: w.State.String(),
		ExpiresAt: w.ExpiresAt.UTC().Format(time.RFC3339),
		LastEvent: w.LastEventSummary,
	}
	if w.LastEventAt != nil {
		at := w.LastEventAt.UTC().Format(time.RFC3339)
		v.LastEventAt = &at
	}
	return v
}

// Create registers one watch, after Control authorizes it and the source is
// proven to exist in the requesting Space.
//
// Order matters: authority FIRST, then the source. A caller who may not watch
// this Space at all must not be able to use the source check as a probe for
// which resources exist in it.
func (h *Handler) Create(w http.ResponseWriter, r *http.Request, orgID, subjectID string) {
	var request createRequest
	if err := json.NewDecoder(http.MaxBytesReader(w, r.Body, 16<<10)).Decode(&request); err != nil {
		writeError(w, http.StatusBadRequest, "a watch request must be a JSON object")
		return
	}
	predicate := Predicate{
		Kind:   strings.TrimSpace(request.Predicate.Kind),
		Value:  request.Predicate.Value,
		Stream: strings.TrimSpace(request.Predicate.Stream),
	}
	if err := predicate.Validate(); err != nil {
		// The caller's own mistake, named. A closed grammar is only useful if a
		// caller can tell which rule they broke.
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}

	ttl := DefaultTTL
	if request.TTLSeconds > 0 {
		ttl = time.Duration(request.TTLSeconds) * time.Second
	}
	if ttl > MaxTTL {
		ttl = MaxTTL
	}

	candidate := Watch{
		ID:               h.newID(),
		OrgID:            orgID,
		SpaceRef:         strings.TrimSpace(request.SpaceRef),
		CreatorSubjectID: subjectID,
		SourceKind:       strings.TrimSpace(request.SourceKind),
		SourceRef:        strings.TrimSpace(request.SourceRef),
		Predicate:        predicate,
		State:            StateActive,
		TriggerMode:      TriggerOnce,
		ExpiresAt:        h.nowFn().Add(ttl),
	}
	if err := candidate.Validate(); err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}

	authority, err := h.authz.AuthorizeCreate(r.Context(), candidate)
	if err != nil {
		writeError(w, http.StatusForbidden, "creating a watch in this Space is not authorized")
		return
	}
	candidate.Authority = authority

	resolver, ok := h.resolvers[candidate.SourceKind]
	if !ok {
		writeError(w, http.StatusBadRequest,
			fmt.Sprintf("%q is not something this deployment can watch", candidate.SourceKind))
		return
	}
	cursor, err := resolver.Resolve(r.Context(), candidate)
	if err != nil {
		// Indistinguishable from "no such resource" by construction — the
		// resolver returns one error for both, so a create cannot become an
		// oracle for which ids exist in an organization.
		writeError(w, http.StatusNotFound, "no such source in this Space")
		return
	}
	candidate.CursorValue = cursor

	active, err := h.store.CountActiveForSpace(r.Context(), orgID, candidate.SpaceRef)
	if err != nil {
		writeError(w, http.StatusServiceUnavailable, "the watch registry is unavailable")
		return
	}
	if active >= MaxActiveWatchesPerSpace {
		writeError(w, http.StatusConflict,
			fmt.Sprintf("this Space already has %d active watches; cancel one before adding another", active))
		return
	}

	created, err := h.store.Create(r.Context(), candidate)
	if errors.Is(err, ErrDuplicateActiveWatch) {
		// Not an error to the person: they asked to be told about something and
		// they already will be. Showing the existing watch is the useful answer.
		writeError(w, http.StatusConflict, "an identical watch is already active in this Space")
		return
	}
	if err != nil {
		writeError(w, http.StatusServiceUnavailable, "the watch could not be recorded")
		return
	}
	writeJSON(w, http.StatusCreated, map[string]any{"data": view(created)})
}

// List returns a Space's watches.
func (h *Handler) List(w http.ResponseWriter, r *http.Request, orgID, spaceRef string, includeFinished bool) {
	spaceRef = strings.TrimSpace(spaceRef)
	if spaceRef == "" {
		writeError(w, http.StatusBadRequest, "space_ref is required")
		return
	}
	found, err := h.store.ListForSpace(r.Context(), orgID, spaceRef, includeFinished)
	if err != nil {
		writeError(w, http.StatusServiceUnavailable, "the watch registry is unavailable")
		return
	}
	views := make([]watchView, 0, len(found))
	for i := range found {
		views = append(views, view(&found[i]))
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": map[string]any{"watches": views}})
}

// Cancel ends one watch.
//
// Idempotent: cancelling an already-terminal watch succeeds, because the
// caller's intent — that this watch stops — is already satisfied, and reporting
// a failure would invite a retry that can never succeed.
func (h *Handler) Cancel(w http.ResponseWriter, r *http.Request, orgID, subjectID, watchID string) {
	existing, err := h.store.Get(r.Context(), orgID, watchID)
	if errors.Is(err, ErrWatchNotFound) {
		writeError(w, http.StatusNotFound, "no such watch")
		return
	}
	if err != nil {
		writeError(w, http.StatusServiceUnavailable, "the watch registry is unavailable")
		return
	}
	if existing.CreatorSubjectID != subjectID {
		// Only the person who asked to be told may stop being told. Cancelling
		// someone else's watch is a write against their standing intent, which
		// is a different authority question from reading the room — and one
		// this slice does not answer.
		//
		// Reported as not-found rather than forbidden: the id belongs to
		// someone, and saying which is a disclosure of its own.
		writeError(w, http.StatusNotFound, "no such watch")
		return
	}
	if err := h.store.Terminate(r.Context(), orgID, watchID, StateCancelled, "cancelled by its creator"); err != nil {
		writeError(w, http.StatusServiceUnavailable, "the watch could not be cancelled")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": map[string]any{"cancelled": true}})
}

func writeJSON(w http.ResponseWriter, status int, body any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(body)
}

func writeError(w http.ResponseWriter, status int, message string) {
	writeJSON(w, status, map[string]any{"error": map[string]any{"message": message}})
}
