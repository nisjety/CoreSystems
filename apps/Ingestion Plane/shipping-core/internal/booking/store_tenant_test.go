package booking

import (
	"context"
	"errors"
	"testing"

	"github.com/jackc/pgx/v5"
	pgxmock "github.com/pashagolub/pgxmock/v4"
)

func TestGetBookingAlwaysPinsOrganization(t *testing.T) {
	db, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("new pgx mock: %v", err)
	}
	defer db.Close()
	db.ExpectQuery(`FROM bookings WHERE id = \$1 AND org_id = \$2`).
		WithArgs("booking-test", "org-a").
		WillReturnError(pgx.ErrNoRows)

	_, err = (&Store{pool: db}).GetBooking(context.Background(), "org-a", "booking-test")
	if !errors.Is(err, ErrNotFound) {
		t.Fatalf("error = %v, want ErrNotFound", err)
	}
	if err := db.ExpectationsWereMet(); err != nil {
		t.Fatal(err)
	}
}

func TestConfirmationClaimPinsOrganizationAndActor(t *testing.T) {
	db, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("new pgx mock: %v", err)
	}
	defer db.Close()
	db.ExpectExec(`(?s)UPDATE bookings.*booked_by = \$4.*approval_id IS NOT NULL`).
		WithArgs("booking-test", "org-a", "single-use-token", "user-a").
		WillReturnResult(pgxmock.NewResult("UPDATE", 0))

	err = (&Store{pool: db}).ClaimForConfirmation(
		context.Background(),
		"org-a",
		"booking-test",
		"single-use-token",
		"user-a",
	)
	if !errors.Is(err, ErrGate) {
		t.Fatalf("error = %v, want ErrGate", err)
	}
	if err := db.ExpectationsWereMet(); err != nil {
		t.Fatal(err)
	}
}

func TestListBookingsAlwaysPinsOrganization(t *testing.T) {
	db, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("new pgx mock: %v", err)
	}
	defer db.Close()
	db.ExpectQuery(`WHERE org_id = \$1 AND \(\$2 = '' OR status = \$2\)`).
		WithArgs("org-a", "", 50).
		WillReturnRows(pgxmock.NewRows([]string{"id"}))

	records, err := (&Store{pool: db}).ListBookings(context.Background(), "org-a", "", 50)
	if err != nil {
		t.Fatalf("ListBookings: %v", err)
	}
	if len(records) != 0 {
		t.Fatalf("records = %d, want 0", len(records))
	}
	if err := db.ExpectationsWereMet(); err != nil {
		t.Fatal(err)
	}
}
