package learning

import (
	"context"
	"embed"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/jackc/pgx/v5/pgconn"
	"io/fs"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

//go:embed migrations/*.sql
var migrationFiles embed.FS

var (
	ErrNotFound            = errors.New("learning record not found")
	ErrForbidden           = errors.New("learning role does not permit this operation")
	ErrConflict            = errors.New("learning record conflicts with its current state")
	ErrLastTeacher         = errors.New("a class must retain at least one active teacher")
	ErrMemberConflict      = errors.New("class member already has a different active role")
	ErrIdempotencyConflict = errors.New("idempotency key was already used for a different request")
	ErrAttemptLimit        = errors.New("attempt event limit reached")
	ErrInvalid             = errors.New("invalid learning record")
)

type Repository struct {
	app       *pgxpool.Pool
	migration *pgxpool.Pool
}

type Class struct {
	ID        string    `json:"id"`
	Name      string    `json:"name"`
	Role      string    `json:"role"`
	CreatedAt time.Time `json:"createdAt"`
}

type ClassMember struct {
	UserID  string    `json:"userId"`
	Role    string    `json:"role"`
	AddedAt time.Time `json:"addedAt"`
}

type Course struct {
	ID          string          `json:"id"`
	ClassID     string          `json:"classId"`
	Revision    int             `json:"revision"`
	Title       string          `json:"title"`
	Description string          `json:"description"`
	Content     json.RawMessage `json:"content"`
	State       string          `json:"state"`
	CreatedAt   time.Time       `json:"createdAt"`
	PublishedAt *time.Time      `json:"publishedAt,omitempty"`
}

type Assignment struct {
	ID        string     `json:"id"`
	ClassID   string     `json:"classId"`
	CourseID  string     `json:"courseId"`
	Revision  int        `json:"revision"`
	Title     string     `json:"title"`
	DueAt     *time.Time `json:"dueAt,omitempty"`
	CreatedAt time.Time  `json:"createdAt"`
}

func Open(ctx context.Context, appURL, migrationURL string) (*Repository, error) {
	appConfig, err := pgxpool.ParseConfig(strings.TrimSpace(appURL))
	if err != nil {
		return nil, fmt.Errorf("parse Learning database URL: %w", err)
	}
	appConfig.MaxConns = 12
	appConfig.MinConns = 1
	app, err := pgxpool.NewWithConfig(ctx, appConfig)
	if err != nil {
		return nil, fmt.Errorf("connect Learning database: %w", err)
	}
	if err := app.Ping(ctx); err != nil {
		app.Close()
		return nil, fmt.Errorf("Learning database is unavailable: %w", err)
	}

	migrationConfig, err := pgxpool.ParseConfig(strings.TrimSpace(migrationURL))
	if err != nil {
		app.Close()
		return nil, fmt.Errorf("parse Learning migration database URL: %w", err)
	}
	migrationConfig.MaxConns = 2
	migrationConfig.ConnConfig.DefaultQueryExecMode = pgx.QueryExecModeSimpleProtocol
	migration, err := pgxpool.NewWithConfig(ctx, migrationConfig)
	if err != nil {
		app.Close()
		return nil, fmt.Errorf("connect Learning migration database: %w", err)
	}
	if err := migration.Ping(ctx); err != nil {
		migration.Close()
		app.Close()
		return nil, fmt.Errorf("Learning migration database is unavailable: %w", err)
	}
	repository := &Repository{app: app, migration: migration}
	if err := repository.Migrate(ctx); err != nil {
		repository.Close()
		return nil, err
	}
	return repository, nil
}

func (r *Repository) Close() {
	if r == nil {
		return
	}
	if r.app != nil {
		r.app.Close()
	}
	if r.migration != nil {
		r.migration.Close()
	}
}

func (r *Repository) Ping(ctx context.Context) error {
	if r == nil || r.app == nil {
		return errors.New("Learning database is not configured")
	}
	return r.app.Ping(ctx)
}

func (r *Repository) Migrate(ctx context.Context) error {
	entries, err := fs.Glob(migrationFiles, "migrations/*.sql")
	if err != nil {
		return fmt.Errorf("list Learning migrations: %w", err)
	}
	if len(entries) == 0 {
		return errors.New("no Learning database migrations were embedded")
	}
	conn, err := r.migration.Acquire(ctx)
	if err != nil {
		return fmt.Errorf("acquire Learning migration connection: %w", err)
	}
	defer conn.Release()
	tx, err := conn.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin Learning migration transaction: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if _, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock(hashtext('learning-plane-schema'))`); err != nil {
		return fmt.Errorf("lock Learning migrations: %w", err)
	}
	if _, err := tx.Exec(ctx, `CREATE TABLE IF NOT EXISTS learning_schema_migrations
(name text PRIMARY KEY, applied_at timestamptz NOT NULL DEFAULT now())`); err != nil {
		return fmt.Errorf("initialize Learning migration history: %w", err)
	}
	for _, name := range entries {
		var applied bool
		if err := tx.QueryRow(ctx, `SELECT EXISTS (SELECT 1 FROM learning_schema_migrations WHERE name=$1)`, name).Scan(&applied); err != nil {
			return fmt.Errorf("read Learning migration history: %w", err)
		}
		if applied {
			continue
		}
		contents, err := migrationFiles.ReadFile(name)
		if err != nil {
			return fmt.Errorf("read Learning migration %s: %w", name, err)
		}
		if _, err := tx.Exec(ctx, string(contents)); err != nil {
			return fmt.Errorf("apply Learning migration %s: %w", name, err)
		}
		if _, err := tx.Exec(ctx, `INSERT INTO learning_schema_migrations(name) VALUES ($1)`, name); err != nil {
			return fmt.Errorf("record Learning migration %s: %w", name, err)
		}
	}
	return tx.Commit(ctx)
}

func (r *Repository) ListClasses(ctx context.Context, orgID, userID string) ([]Class, error) {
	var classes []Class
	err := r.withOrg(ctx, orgID, func(tx pgx.Tx) error {
		rows, err := tx.Query(ctx, `
SELECT c.id, c.name, m.role, c.created_at
FROM learning_classes c
JOIN learning_class_members m ON m.org_id=c.org_id AND m.class_id=c.id
WHERE c.org_id=$1 AND m.user_id=$2 AND m.active
ORDER BY c.created_at DESC, c.id LIMIT 200`, orgID, userID)
		if err != nil {
			return err
		}
		defer rows.Close()
		for rows.Next() {
			var class Class
			if err := rows.Scan(&class.ID, &class.Name, &class.Role, &class.CreatedAt); err != nil {
				return err
			}
			classes = append(classes, class)
		}
		return rows.Err()
	})
	if classes == nil {
		classes = []Class{}
	}
	return classes, err
}

func (r *Repository) CreateClass(ctx context.Context, orgID, userID, name string) (Class, error) {
	var class Class
	err := r.withOrg(ctx, orgID, func(tx pgx.Tx) error {
		err := tx.QueryRow(ctx, `INSERT INTO learning_classes (org_id, name, created_by)
VALUES ($1, $2, $3) RETURNING id, name, created_at`, orgID, strings.TrimSpace(name), userID).
			Scan(&class.ID, &class.Name, &class.CreatedAt)
		if err != nil {
			return err
		}
		if _, err := tx.Exec(ctx, `INSERT INTO learning_class_members (org_id, class_id, user_id, role)
VALUES ($1, $2, $3, 'teacher')`, orgID, class.ID, userID); err != nil {
			return err
		}
		class.Role = "teacher"
		return nil
	})
	return class, err
}

func (r *Repository) ClassRole(ctx context.Context, orgID, classID, userID string) (string, error) {
	var role string
	err := r.withOrg(ctx, orgID, func(tx pgx.Tx) error {
		err := tx.QueryRow(ctx, `SELECT role FROM learning_class_members
WHERE org_id=$1 AND class_id=$2 AND user_id=$3 AND active`, orgID, classID, userID).Scan(&role)
		if errors.Is(err, pgx.ErrNoRows) {
			return ErrNotFound
		}
		return err
	})
	return role, err
}

func (r *Repository) ListClassMembers(ctx context.Context, orgID, classID, viewerID string) ([]ClassMember, error) {
	var members []ClassMember
	err := r.withOrg(ctx, orgID, func(tx pgx.Tx) error {
		rows, err := tx.Query(ctx, `SELECT user_id,role,created_at FROM learning_class_members
WHERE org_id=$1 AND class_id=$2 AND active
  AND EXISTS (SELECT 1 FROM learning_class_members viewer
    WHERE viewer.org_id=$1 AND viewer.class_id=$2 AND viewer.user_id=$3
      AND viewer.active AND viewer.role='teacher')
ORDER BY role,user_id LIMIT 500`, orgID, classID, viewerID)
		if err != nil {
			return err
		}
		defer rows.Close()
		for rows.Next() {
			var member ClassMember
			if err := rows.Scan(&member.UserID, &member.Role, &member.AddedAt); err != nil {
				return err
			}
			members = append(members, member)
		}
		return rows.Err()
	})
	if members == nil {
		members = []ClassMember{}
	}
	return members, err
}

func (r *Repository) AddClassMember(ctx context.Context, orgID, classID, actorID, userID, role string, organizationAdmin bool) error {
	return r.withOrg(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockClass(ctx, tx, orgID, classID); err != nil {
			return err
		}
		actorRole, err := activeClassRole(ctx, tx, orgID, classID, actorID)
		if err != nil {
			return err
		}
		if !organizationAdmin && (actorRole != "teacher" || role != "student") {
			return ErrForbidden
		}
		tag, err := tx.Exec(ctx, `INSERT INTO learning_class_members (org_id,class_id,user_id,role)
VALUES ($1,$2,$3,$4) ON CONFLICT (org_id,class_id,user_id)
DO UPDATE SET role=EXCLUDED.role, active=true
WHERE NOT learning_class_members.active OR learning_class_members.role=EXCLUDED.role`, orgID, classID, userID, role)
		if err == nil && tag.RowsAffected() == 0 {
			return ErrMemberConflict
		}
		var postgresError *pgconn.PgError
		if errors.As(err, &postgresError) && postgresError.Code == "23503" {
			return ErrNotFound
		}
		return err
	})
}

func (r *Repository) RemoveClassMember(ctx context.Context, orgID, classID, actorID, userID string, organizationAdmin bool) error {
	return r.withOrg(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockClass(ctx, tx, orgID, classID); err != nil {
			return err
		}
		actorRole, err := activeClassRole(ctx, tx, orgID, classID, actorID)
		if err != nil {
			return err
		}
		if !organizationAdmin && (actorRole != "teacher" || userID != actorID) {
			return ErrForbidden
		}
		var role string
		err = tx.QueryRow(ctx, `SELECT role FROM learning_class_members
WHERE org_id=$1 AND class_id=$2 AND user_id=$3 AND active FOR UPDATE`, orgID, classID, userID).Scan(&role)
		if errors.Is(err, pgx.ErrNoRows) {
			return ErrNotFound
		}
		if err != nil {
			return err
		}
		if role == "teacher" {
			var teachers int
			if err := tx.QueryRow(ctx, `SELECT count(*) FROM learning_class_members
WHERE org_id=$1 AND class_id=$2 AND role='teacher' AND active`, orgID, classID).Scan(&teachers); err != nil {
				return err
			}
			if teachers <= 1 {
				return ErrLastTeacher
			}
		}
		_, err = tx.Exec(ctx, `UPDATE learning_class_members SET active=false
WHERE org_id=$1 AND class_id=$2 AND user_id=$3 AND active`, orgID, classID, userID)
		return err
	})
}

func lockClass(ctx context.Context, tx pgx.Tx, orgID, classID string) error {
	var id string
	err := tx.QueryRow(ctx, `SELECT id FROM learning_classes WHERE org_id=$1 AND id=$2 FOR UPDATE`, orgID, classID).Scan(&id)
	if errors.Is(err, pgx.ErrNoRows) {
		return ErrNotFound
	}
	return err
}

func activeClassRole(ctx context.Context, tx pgx.Tx, orgID, classID, userID string) (string, error) {
	var role string
	err := tx.QueryRow(ctx, `SELECT role FROM learning_class_members
WHERE org_id=$1 AND class_id=$2 AND user_id=$3 AND active`, orgID, classID, userID).Scan(&role)
	if errors.Is(err, pgx.ErrNoRows) {
		return "", ErrNotFound
	}
	return role, err
}

func requireClassTeacher(ctx context.Context, tx pgx.Tx, orgID, classID, userID string) error {
	role, err := activeClassRole(ctx, tx, orgID, classID, userID)
	if err != nil {
		return err
	}
	if role != "teacher" {
		return ErrForbidden
	}
	return nil
}

func (r *Repository) CreateCourse(ctx context.Context, orgID, classID, userID, title, description string, content json.RawMessage) (Course, error) {
	var course Course
	err := r.withOrg(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockClass(ctx, tx, orgID, classID); err != nil {
			return err
		}
		if err := requireClassTeacher(ctx, tx, orgID, classID, userID); err != nil {
			return err
		}
		err := tx.QueryRow(ctx, `INSERT INTO learning_courses(org_id,class_id,created_by)
VALUES($1,$2,$3) RETURNING id`, orgID, classID, userID).Scan(&course.ID)
		if err != nil {
			return err
		}
		err = tx.QueryRow(ctx, `INSERT INTO learning_course_revisions
(course_id,org_id,class_id,revision,title,description,content,authored_by)
VALUES($1,$2,$3,1,$4,$5,$6,$7) RETURNING revision,state,created_at`,
			course.ID, orgID, classID, strings.TrimSpace(title), strings.TrimSpace(description), content, userID).
			Scan(&course.Revision, &course.State, &course.CreatedAt)
		course.ClassID, course.Title, course.Description, course.Content = classID, strings.TrimSpace(title), strings.TrimSpace(description), append(json.RawMessage(nil), content...)
		return err
	})
	return course, err
}

func (r *Repository) CreateRevision(ctx context.Context, orgID, classID, courseID, userID, title, description string, content json.RawMessage) (Course, error) {
	var course Course
	err := r.withOrg(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockClass(ctx, tx, orgID, classID); err != nil {
			return err
		}
		if err := requireClassTeacher(ctx, tx, orgID, classID, userID); err != nil {
			return err
		}
		var existing string
		if err := tx.QueryRow(ctx, `SELECT id FROM learning_courses
WHERE org_id=$1 AND class_id=$2 AND id=$3 FOR UPDATE`, orgID, classID, courseID).Scan(&existing); errors.Is(err, pgx.ErrNoRows) {
			return ErrNotFound
		} else if err != nil {
			return err
		}
		var latest int
		var draftExists bool
		if err := tx.QueryRow(ctx, `SELECT COALESCE(MAX(revision),0), COALESCE(bool_or(state='draft'),false)
FROM learning_course_revisions WHERE org_id=$1 AND class_id=$2 AND course_id=$3`, orgID, classID, courseID).Scan(&latest, &draftExists); err != nil {
			return err
		}
		if draftExists {
			return ErrConflict
		}
		next := latest + 1
		err := tx.QueryRow(ctx, `INSERT INTO learning_course_revisions
(course_id,org_id,class_id,revision,title,description,content,authored_by)
VALUES($1,$2,$3,$4,$5,$6,$7,$8) RETURNING state,created_at`,
			courseID, orgID, classID, next, strings.TrimSpace(title), strings.TrimSpace(description), content, userID).
			Scan(&course.State, &course.CreatedAt)
		course.ID, course.ClassID, course.Revision = courseID, classID, next
		course.Title, course.Description, course.Content = strings.TrimSpace(title), strings.TrimSpace(description), append(json.RawMessage(nil), content...)
		return err
	})
	return course, err
}

func (r *Repository) PublishCourse(ctx context.Context, orgID, classID, courseID, userID string, revision int) (Course, error) {
	var course Course
	err := r.withOrg(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockClass(ctx, tx, orgID, classID); err != nil {
			return err
		}
		if err := requireClassTeacher(ctx, tx, orgID, classID, userID); err != nil {
			return err
		}
		var state string
		err := tx.QueryRow(ctx, `SELECT title,description,content,state,created_at
FROM learning_course_revisions WHERE org_id=$1 AND class_id=$2 AND course_id=$3 AND revision=$4 FOR UPDATE`,
			orgID, classID, courseID, revision).Scan(&course.Title, &course.Description, &course.Content, &state, &course.CreatedAt)
		if errors.Is(err, pgx.ErrNoRows) {
			return ErrNotFound
		}
		if err != nil {
			return err
		}
		if state != "draft" {
			return ErrConflict
		}
		if err := tx.QueryRow(ctx, `UPDATE learning_course_revisions SET state='published', published_at=now()
WHERE org_id=$1 AND class_id=$2 AND course_id=$3 AND revision=$4 RETURNING published_at`,
			orgID, classID, courseID, revision).Scan(&course.PublishedAt); err != nil {
			return err
		}
		course.ID, course.ClassID, course.Revision, course.State = courseID, classID, revision, "published"
		return nil
	})
	return course, err
}

func (r *Repository) CreateAssignment(ctx context.Context, orgID, classID, courseID, userID string, revision int, dueAt *time.Time) (Assignment, error) {
	var assignment Assignment
	err := r.withOrg(ctx, orgID, func(tx pgx.Tx) error {
		if err := lockClass(ctx, tx, orgID, classID); err != nil {
			return err
		}
		if err := requireClassTeacher(ctx, tx, orgID, classID, userID); err != nil {
			return err
		}
		var title string
		err := tx.QueryRow(ctx, `SELECT title FROM learning_course_revisions
WHERE org_id=$1 AND class_id=$2 AND course_id=$3 AND revision=$4 AND state='published'`,
			orgID, classID, courseID, revision).Scan(&title)
		if errors.Is(err, pgx.ErrNoRows) {
			return ErrNotFound
		}
		if err != nil {
			return err
		}
		err = tx.QueryRow(ctx, `INSERT INTO learning_assignments
(org_id,class_id,course_id,course_revision,assigned_by,due_at)
VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT (org_id,class_id,course_id,course_revision)
		DO UPDATE SET id=learning_assignments.id RETURNING id,created_at,due_at`,
			orgID, classID, courseID, revision, userID, dueAt).Scan(&assignment.ID, &assignment.CreatedAt, &assignment.DueAt)
		assignment.ClassID, assignment.CourseID, assignment.Revision, assignment.Title = classID, courseID, revision, title
		return err
	})
	return assignment, err
}

func (r *Repository) ListAssignments(ctx context.Context, orgID, classID, userID, classRole string) ([]Assignment, error) {
	var assignments []Assignment
	err := r.withOrg(ctx, orgID, func(tx pgx.Tx) error {
		query := `SELECT a.id,a.class_id,a.course_id,a.course_revision,r.title,a.due_at,a.created_at
FROM learning_assignments a
JOIN learning_course_revisions r ON r.org_id=a.org_id AND r.course_id=a.course_id AND r.revision=a.course_revision
WHERE a.org_id=$1 AND a.class_id=$2 AND EXISTS (
  SELECT 1 FROM learning_class_members m WHERE m.org_id=a.org_id AND m.class_id=a.class_id AND m.user_id=$3 AND m.active AND m.role=$4
)
ORDER BY a.created_at DESC,a.id LIMIT 500`
		rows, err := tx.Query(ctx, query, orgID, classID, userID, classRole)
		if err != nil {
			return err
		}
		defer rows.Close()
		for rows.Next() {
			var a Assignment
			if err := rows.Scan(&a.ID, &a.ClassID, &a.CourseID, &a.Revision, &a.Title, &a.DueAt, &a.CreatedAt); err != nil {
				return err
			}
			assignments = append(assignments, a)
		}
		return rows.Err()
	})
	if assignments == nil {
		assignments = []Assignment{}
	}
	return assignments, err
}

func (r *Repository) ListCourses(ctx context.Context, orgID, classID, userID, classRole string) ([]Course, error) {
	var courses []Course
	err := r.withOrg(ctx, orgID, func(tx pgx.Tx) error {
		query := `SELECT r.course_id,r.class_id,r.revision,r.title,r.description,r.content,r.state,r.created_at,r.published_at
FROM learning_course_revisions r
WHERE r.org_id=$1 AND r.class_id=$2 AND (
  ($4='teacher' AND r.state IN ('draft','published')) OR
  ($4='student' AND r.state='published' AND EXISTS (
    SELECT 1 FROM learning_assignments a WHERE a.org_id=r.org_id AND a.class_id=r.class_id
      AND a.course_id=r.course_id AND a.course_revision=r.revision
  ))
)
AND (r.state='published' OR NOT EXISTS (
  SELECT 1 FROM learning_course_revisions newer WHERE newer.org_id=r.org_id
    AND newer.class_id=r.class_id AND newer.course_id=r.course_id AND newer.revision>r.revision
))
AND EXISTS (SELECT 1 FROM learning_class_members m WHERE m.org_id=r.org_id AND m.class_id=r.class_id AND m.user_id=$3 AND m.active)
ORDER BY r.created_at DESC,r.course_id LIMIT 500`
		rows, err := tx.Query(ctx, query, orgID, classID, userID, classRole)
		if err != nil {
			return err
		}
		defer rows.Close()
		for rows.Next() {
			var c Course
			if err := rows.Scan(&c.ID, &c.ClassID, &c.Revision, &c.Title, &c.Description, &c.Content, &c.State, &c.CreatedAt, &c.PublishedAt); err != nil {
				return err
			}
			courses = append(courses, c)
		}
		return rows.Err()
	})
	if courses == nil {
		courses = []Course{}
	}
	return courses, err
}

func (r *Repository) withOrg(ctx context.Context, orgID string, operation func(pgx.Tx) error) error {
	if r == nil || r.app == nil {
		return errors.New("Learning database is not configured")
	}
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return ErrForbidden
	}
	tx, err := r.app.Begin(ctx)
	if err != nil {
		return err
	}
	defer func() { _ = tx.Rollback(context.Background()) }()
	if _, err := tx.Exec(ctx, `SELECT set_config('app.current_org_id',$1,true)`, orgID); err != nil {
		return err
	}
	if err := operation(tx); err != nil {
		return err
	}
	return tx.Commit(ctx)
}
