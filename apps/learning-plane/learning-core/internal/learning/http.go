package learning

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strconv"
	"strings"
	"time"

	controlauth "coresystem/learning-core/internal/auth"
)

type identityContextKey struct{}

type API struct {
	authority  *controlauth.Authority
	repository *Repository
}

func NewHandler(authority *controlauth.Authority, repository *Repository) http.Handler {
	api := &API{authority: authority, repository: repository}
	mux := http.NewServeMux()
	mux.HandleFunc("GET /healthz", func(w http.ResponseWriter, r *http.Request) {
		writeJSON(w, http.StatusOK, map[string]any{"data": map[string]string{"status": "alive"}})
	})
	mux.Handle("GET /readyz", http.HandlerFunc(api.readiness))
	mux.Handle("GET /api/v1/learning/status", http.HandlerFunc(api.status))
	mux.Handle("GET /api/v1/learning/me", api.authenticated(api.me))
	mux.Handle("GET /api/v1/learning/classes", api.authenticated(api.listClasses))
	mux.Handle("POST /api/v1/learning/classes", api.authenticated(api.createClass))
	mux.Handle("GET /api/v1/learning/classes/{classID}/members", api.authenticated(api.listMembers))
	mux.Handle("POST /api/v1/learning/classes/{classID}/members", api.authenticated(api.addMember))
	mux.Handle("DELETE /api/v1/learning/classes/{classID}/members/{userID}", api.authenticated(api.removeMember))
	mux.Handle("GET /api/v1/learning/classes/{classID}/courses", api.authenticated(api.listCourses))
	mux.Handle("POST /api/v1/learning/classes/{classID}/courses", api.authenticated(api.createCourse))
	mux.Handle("POST /api/v1/learning/classes/{classID}/courses/{courseID}/revisions", api.authenticated(api.createRevision))
	mux.Handle("POST /api/v1/learning/classes/{classID}/courses/{courseID}/revisions/{revision}/publish", api.authenticated(api.publishCourse))
	mux.Handle("POST /api/v1/learning/classes/{classID}/assignments", api.authenticated(api.createAssignment))
	mux.Handle("GET /api/v1/learning/classes/{classID}/assignments", api.authenticated(api.listAssignments))
	mux.Handle("POST /api/v1/learning/classes/{classID}/assignments/{assignmentID}/attempts", api.authenticated(api.createAttempt))
	mux.Handle("GET /api/v1/learning/classes/{classID}/assignments/{assignmentID}/attempts", api.authenticated(api.listOwnAttempts))
	mux.Handle("GET /api/v1/learning/classes/{classID}/assignments/{assignmentID}/progress", api.authenticated(api.listAssignmentProgress))
	mux.Handle("POST /api/v1/learning/classes/{classID}/attempts/{attemptID}/events", api.authenticated(api.recordAttemptEvent))
	mux.Handle("GET /api/v1/learning/classes/{classID}/attempts/{attemptID}/events", api.authenticated(api.listAttemptEvents))
	return securedHeaders(mux)
}

func (api *API) authenticated(next http.HandlerFunc) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet && r.Header.Get("X-Learning-Request") != "dashboard-v1" {
			writeAPIError(w, http.StatusForbidden, "SAME_ORIGIN_REQUEST_REQUIRED", "This operation must come from the signed-in Learning workspace.")
			return
		}
		ctx, cancel := context.WithTimeout(r.Context(), 4*time.Second)
		defer cancel()
		identity, err := api.authority.Resolve(ctx, r.Header.Get("Authorization"), r.Header.Get("Cookie"))
		if err != nil {
			switch {
			case errors.Is(err, controlauth.ErrUnauthenticated):
				writeAPIError(w, http.StatusUnauthorized, "AUTHENTICATION_REQUIRED", "Sign in with your school account.")
			case errors.Is(err, controlauth.ErrForbidden):
				writeAPIError(w, http.StatusForbidden, "SCHOOL_MEMBERSHIP_REQUIRED", "Your account is not an active member of this school.")
			default:
				writeAPIError(w, http.StatusServiceUnavailable, "CONTROL_AUTHORITY_UNAVAILABLE", "School access could not be verified.")
			}
			return
		}
		next.ServeHTTP(w, r.WithContext(context.WithValue(ctx, identityContextKey{}, identity)))
	})
}

func identityFrom(ctx context.Context) controlauth.Identity {
	identity, _ := ctx.Value(identityContextKey{}).(controlauth.Identity)
	return identity
}

func (api *API) status(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, http.StatusOK, map[string]any{"data": map[string]any{
		"service": "learning-core", "phase": "school-access-and-courses", "ready": false,
		"processingProfile": "strict-self-hosted",
		"capabilities": []map[string]string{
			{"key": "school-identity", "state": "connected"},
			{"key": "course-persistence", "state": "connected"},
			{"key": "learner-progress", "state": "connected"},
			{"key": "self-hosted-speech", "state": "not-connected"},
			{"key": "tutor-runtime", "state": "not-connected"},
			{"key": "teacher-assistance", "state": "not-connected"},
		},
	}})
}

func (api *API) readiness(w http.ResponseWriter, r *http.Request) {
	ctx, cancel := context.WithTimeout(r.Context(), time.Second)
	defer cancel()
	if api.authority == nil || api.repository == nil || api.authority.Ping(ctx) != nil || api.repository.Ping(ctx) != nil {
		writeAPIError(w, http.StatusServiceUnavailable, "LEARNING_NOT_READY", "School access or course storage is unavailable.")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": map[string]string{"status": "school-courses-ready"}})
}

func (api *API) me(w http.ResponseWriter, r *http.Request) {
	identity := identityFrom(r.Context())
	writeJSON(w, http.StatusOK, map[string]any{"data": map[string]any{
		"user":         map[string]string{"id": identity.UserID},
		"organization": map[string]string{"id": identity.OrgID, "role": identity.OrgRole},
	}})
}

func (api *API) listClasses(w http.ResponseWriter, r *http.Request) {
	id := identityFrom(r.Context())
	classes, err := api.repository.ListClasses(r.Context(), id.OrgID, id.UserID)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": classes})
}

func (api *API) createClass(w http.ResponseWriter, r *http.Request) {
	id := identityFrom(r.Context())
	if !organizationAdmin(id.OrgRole) {
		writeAPIError(w, http.StatusForbidden, "SCHOOL_ADMIN_REQUIRED", "A school owner or administrator must create classes.")
		return
	}
	var request struct {
		Name string `json:"name"`
	}
	if !decodeRequest(w, r, &request, 8<<10) {
		return
	}
	request.Name = strings.TrimSpace(request.Name)
	if request.Name == "" || len([]rune(request.Name)) > 120 {
		writeAPIError(w, http.StatusBadRequest, "INVALID_CLASS", "Class name must contain 1 to 120 characters.")
		return
	}
	class, err := api.repository.CreateClass(r.Context(), id.OrgID, id.UserID, request.Name)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	writeJSON(w, http.StatusCreated, map[string]any{"data": class})
}

func (api *API) addMember(w http.ResponseWriter, r *http.Request) {
	id := identityFrom(r.Context())
	classID := r.PathValue("classID")
	callerRole, err := api.repository.ClassRole(r.Context(), id.OrgID, classID, id.UserID)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	var request struct {
		UserID string `json:"userId"`
		Role   string `json:"role"`
	}
	if !decodeRequest(w, r, &request, 8<<10) {
		return
	}
	request.UserID, request.Role = strings.TrimSpace(request.UserID), strings.ToLower(strings.TrimSpace(request.Role))
	if request.UserID == "" || len(request.UserID) > 128 || (request.Role != "teacher" && request.Role != "student") {
		writeAPIError(w, http.StatusBadRequest, "INVALID_CLASS_MEMBER", "A school user ID and class role (teacher or student) are required.")
		return
	}
	if !organizationAdmin(id.OrgRole) && (callerRole != "teacher" || request.Role != "student") {
		writeAPIError(w, http.StatusForbidden, "CLASS_TEACHER_REQUIRED", "Teachers may add students; a school administrator must assign teacher access.")
		return
	}
	_, member, err := api.authority.CheckMembership(r.Context(), request.UserID, id.OrgID)
	if err != nil {
		writeAPIError(w, http.StatusServiceUnavailable, "CONTROL_AUTHORITY_UNAVAILABLE", "The requested school member could not be verified.")
		return
	}
	if !member {
		writeAPIError(w, http.StatusNotFound, "SCHOOL_MEMBER_NOT_FOUND", "The requested account is not an active member of this school.")
		return
	}
	if err := api.repository.AddClassMember(r.Context(), id.OrgID, classID, id.UserID, request.UserID, request.Role, organizationAdmin(id.OrgRole)); err != nil {
		writeStoreError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": map[string]string{"userId": request.UserID, "role": request.Role, "status": "active"}})
}

func (api *API) listMembers(w http.ResponseWriter, r *http.Request) {
	id := identityFrom(r.Context())
	classID := r.PathValue("classID")
	role, err := api.repository.ClassRole(r.Context(), id.OrgID, classID, id.UserID)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	if role != "teacher" {
		writeAPIError(w, http.StatusForbidden, "CLASS_TEACHER_REQUIRED", "Only a class teacher can view its member roster.")
		return
	}
	members, err := api.repository.ListClassMembers(r.Context(), id.OrgID, classID, id.UserID)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": members})
}

func (api *API) removeMember(w http.ResponseWriter, r *http.Request) {
	id := identityFrom(r.Context())
	classID, userID := r.PathValue("classID"), r.PathValue("userID")
	callerRole, err := api.repository.ClassRole(r.Context(), id.OrgID, classID, id.UserID)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	if !organizationAdmin(id.OrgRole) && (callerRole != "teacher" || userID != id.UserID) {
		writeAPIError(w, http.StatusForbidden, "CLASS_TEACHER_REQUIRED", "A school administrator or the teacher themselves must update class membership.")
		return
	}
	if err := api.repository.RemoveClassMember(r.Context(), id.OrgID, classID, id.UserID, userID, organizationAdmin(id.OrgRole)); err != nil {
		writeStoreError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": map[string]bool{"removed": true}})
}

func (api *API) listCourses(w http.ResponseWriter, r *http.Request) {
	id := identityFrom(r.Context())
	classID := r.PathValue("classID")
	role, err := api.repository.ClassRole(r.Context(), id.OrgID, classID, id.UserID)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	courses, err := api.repository.ListCourses(r.Context(), id.OrgID, classID, id.UserID, role)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": courses})
}

type courseInput struct {
	Title       string          `json:"title"`
	Description string          `json:"description"`
	Content     json.RawMessage `json:"content"`
}

func validateCourseInput(input courseInput) bool {
	return strings.TrimSpace(input.Title) != "" && len([]rune(input.Title)) <= 160 &&
		len(input.Description) <= 2000 && len(input.Content) > 1 &&
		json.Valid(input.Content) && ValidCourseDefinition(input.Content)
}

func (api *API) createCourse(w http.ResponseWriter, r *http.Request) {
	id := identityFrom(r.Context())
	classID := r.PathValue("classID")
	if !api.requireTeacher(w, r, id.OrgID, classID, id.UserID) {
		return
	}
	var request courseInput
	if !decodeRequest(w, r, &request, 1<<20) {
		return
	}
	if !validateCourseInput(request) {
		writeAPIError(w, http.StatusBadRequest, "INVALID_COURSE", "A title, short description and JSON course definition are required.")
		return
	}
	course, err := api.repository.CreateCourse(r.Context(), id.OrgID, classID, id.UserID, request.Title, request.Description, request.Content)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	writeJSON(w, http.StatusCreated, map[string]any{"data": course})
}

func (api *API) createRevision(w http.ResponseWriter, r *http.Request) {
	id := identityFrom(r.Context())
	classID, courseID := r.PathValue("classID"), r.PathValue("courseID")
	if !api.requireTeacher(w, r, id.OrgID, classID, id.UserID) {
		return
	}
	var request courseInput
	if !decodeRequest(w, r, &request, 1<<20) {
		return
	}
	if !validateCourseInput(request) {
		writeAPIError(w, http.StatusBadRequest, "INVALID_COURSE", "A title, short description and JSON course definition are required.")
		return
	}
	course, err := api.repository.CreateRevision(r.Context(), id.OrgID, classID, courseID, id.UserID, request.Title, request.Description, request.Content)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	writeJSON(w, http.StatusCreated, map[string]any{"data": course})
}

func (api *API) publishCourse(w http.ResponseWriter, r *http.Request) {
	id := identityFrom(r.Context())
	classID, courseID := r.PathValue("classID"), r.PathValue("courseID")
	if !api.requireTeacher(w, r, id.OrgID, classID, id.UserID) {
		return
	}
	revision, err := strconv.Atoi(r.PathValue("revision"))
	if err != nil || revision < 1 {
		writeAPIError(w, http.StatusBadRequest, "INVALID_REVISION", "Course revision must be a positive integer.")
		return
	}
	course, err := api.repository.PublishCourse(r.Context(), id.OrgID, classID, courseID, id.UserID, revision)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": course})
}

func (api *API) createAssignment(w http.ResponseWriter, r *http.Request) {
	id := identityFrom(r.Context())
	classID := r.PathValue("classID")
	if !api.requireTeacher(w, r, id.OrgID, classID, id.UserID) {
		return
	}
	var request struct {
		CourseID string     `json:"courseId"`
		Revision int        `json:"revision"`
		DueAt    *time.Time `json:"dueAt"`
	}
	if !decodeRequest(w, r, &request, 8<<10) {
		return
	}
	request.CourseID = strings.TrimSpace(request.CourseID)
	if request.CourseID == "" || request.Revision < 1 || (request.DueAt != nil && request.DueAt.Before(time.Now().Add(-time.Minute))) {
		writeAPIError(w, http.StatusBadRequest, "INVALID_ASSIGNMENT", "A published course revision and a current optional due date are required.")
		return
	}
	assignment, err := api.repository.CreateAssignment(r.Context(), id.OrgID, classID, request.CourseID, id.UserID, request.Revision, request.DueAt)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	writeJSON(w, http.StatusCreated, map[string]any{"data": assignment})
}

func (api *API) listAssignments(w http.ResponseWriter, r *http.Request) {
	id := identityFrom(r.Context())
	classID := r.PathValue("classID")
	role, err := api.repository.ClassRole(r.Context(), id.OrgID, classID, id.UserID)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	assignments, err := api.repository.ListAssignments(r.Context(), id.OrgID, classID, id.UserID, role)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": assignments})
}

func (api *API) createAttempt(w http.ResponseWriter, r *http.Request) {
	id := identityFrom(r.Context())
	classID, assignmentID := r.PathValue("classID"), r.PathValue("assignmentID")
	var request struct {
		Mode string `json:"mode"`
	}
	if !decodeRequest(w, r, &request, 8<<10) {
		return
	}
	keyHash, err := HashIdempotencyKey(r.Header.Get("Idempotency-Key"))
	if err != nil || !validAttemptMode(request.Mode) {
		writeAPIError(w, http.StatusBadRequest, "INVALID_ATTEMPT", "A valid guide, demonstrate or practice mode and Idempotency-Key are required.")
		return
	}
	attempt, created, err := api.repository.CreateAttempt(r.Context(), id.OrgID, classID, assignmentID, id.UserID, request.Mode, keyHash)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	status := http.StatusOK
	if created {
		status = http.StatusCreated
	}
	writeJSON(w, status, map[string]any{"data": attempt})
}

func (api *API) listOwnAttempts(w http.ResponseWriter, r *http.Request) {
	id := identityFrom(r.Context())
	attempts, err := api.repository.ListOwnAttempts(r.Context(), id.OrgID, r.PathValue("classID"), r.PathValue("assignmentID"), id.UserID)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": attempts})
}

func (api *API) recordAttemptEvent(w http.ResponseWriter, r *http.Request) {
	id := identityFrom(r.Context())
	var request struct {
		Type   string `json:"type"`
		StepID string `json:"stepId"`
	}
	if !decodeRequest(w, r, &request, 8<<10) {
		return
	}
	keyHash, err := HashIdempotencyKey(r.Header.Get("Idempotency-Key"))
	if err != nil {
		writeAPIError(w, http.StatusBadRequest, "IDEMPOTENCY_KEY_REQUIRED", "A valid Idempotency-Key is required.")
		return
	}
	event, created, err := api.repository.RecordAttemptEvent(r.Context(), id.OrgID, r.PathValue("classID"), r.PathValue("attemptID"), id.UserID, request.Type, request.StepID, keyHash)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	status := http.StatusOK
	if created {
		status = http.StatusCreated
	}
	writeJSON(w, status, map[string]any{"data": event})
}

func (api *API) listAttemptEvents(w http.ResponseWriter, r *http.Request) {
	id := identityFrom(r.Context())
	events, err := api.repository.ListAttemptEvents(r.Context(), id.OrgID, r.PathValue("classID"), r.PathValue("attemptID"), id.UserID)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": events})
}

func (api *API) listAssignmentProgress(w http.ResponseWriter, r *http.Request) {
	id := identityFrom(r.Context())
	classID, assignmentID := r.PathValue("classID"), r.PathValue("assignmentID")
	if !api.requireTeacher(w, r, id.OrgID, classID, id.UserID) {
		return
	}
	progress, err := api.repository.ListAssignmentProgress(r.Context(), id.OrgID, classID, assignmentID, id.UserID)
	if err != nil {
		writeStoreError(w, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": progress})
}

func (api *API) requireTeacher(w http.ResponseWriter, r *http.Request, orgID, classID, userID string) bool {
	role, err := api.repository.ClassRole(r.Context(), orgID, classID, userID)
	if err != nil {
		writeStoreError(w, err)
		return false
	}
	if role != "teacher" {
		writeAPIError(w, http.StatusForbidden, "CLASS_TEACHER_REQUIRED", "An active class teacher is required.")
		return false
	}
	return true
}

func organizationAdmin(role string) bool { return role == "owner" || role == "admin" }

func decodeRequest(w http.ResponseWriter, r *http.Request, target any, maxBytes int64) bool {
	r.Body = http.MaxBytesReader(w, r.Body, maxBytes)
	decoder := json.NewDecoder(r.Body)
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(target); err != nil {
		writeAPIError(w, http.StatusBadRequest, "INVALID_REQUEST", "Request body is invalid or exceeds the allowed size.")
		return false
	}
	var trailing any
	if err := decoder.Decode(&trailing); err != io.EOF {
		writeAPIError(w, http.StatusBadRequest, "INVALID_REQUEST", "Request body must contain one JSON value.")
		return false
	}
	return true
}

func writeStoreError(w http.ResponseWriter, err error) {
	switch {
	case errors.Is(err, ErrNotFound):
		writeAPIError(w, http.StatusNotFound, "LEARNING_RECORD_NOT_FOUND", "The requested class or course was not found.")
	case errors.Is(err, ErrForbidden):
		writeAPIError(w, http.StatusForbidden, "CLASS_ACCESS_DENIED", "You do not have access to this class.")
	case errors.Is(err, ErrConflict):
		writeAPIError(w, http.StatusConflict, "LEARNING_STATE_CONFLICT", "The learning record has already moved to a state that does not allow this operation.")
	case errors.Is(err, ErrLastTeacher):
		writeAPIError(w, http.StatusConflict, "CLASS_REQUIRES_TEACHER", "A class must retain at least one active teacher.")
	case errors.Is(err, ErrMemberConflict):
		writeAPIError(w, http.StatusConflict, "CLASS_MEMBER_ROLE_CONFLICT", "This active member already has a different class role.")
	case errors.Is(err, ErrIdempotencyConflict):
		writeAPIError(w, http.StatusConflict, "IDEMPOTENCY_KEY_REUSED", "This Idempotency-Key was already used for a different request.")
	case errors.Is(err, ErrAttemptLimit):
		writeAPIError(w, http.StatusTooManyRequests, "ATTEMPT_EVENT_LIMIT", "This learning attempt has reached its event limit.")
	case errors.Is(err, ErrInvalid):
		writeAPIError(w, http.StatusBadRequest, "INVALID_LEARNING_RECORD", "The learning record does not match the supported contract.")
	default:
		writeAPIError(w, http.StatusServiceUnavailable, "LEARNING_STORAGE_UNAVAILABLE", "Learning records could not be saved or loaded.")
	}
}

func writeAPIError(w http.ResponseWriter, status int, code, message string) {
	writeJSON(w, status, map[string]any{"error": map[string]any{
		"code": code, "message": message, "details": map[string]any{"retryable": status == http.StatusServiceUnavailable},
	}})
}

func writeJSON(w http.ResponseWriter, status int, body any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(body)
}

func securedHeaders(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		w.Header().Set("X-Content-Type-Options", "nosniff")
		next.ServeHTTP(w, r)
	})
}
