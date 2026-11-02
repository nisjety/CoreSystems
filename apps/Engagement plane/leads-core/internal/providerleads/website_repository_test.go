package providerleads

import (
	"bytes"
	"context"
	"github.com/jackc/pgx/v5/pgxpool"
	"os"
	"strings"
	"testing"
	"time"
)

func TestWebsitePostgresReceiptIdempotencyIsolationAndErasure(t *testing.T) {
	address := os.Getenv("WEBSITE_INQUIRIES_TEST_DATABASE_URL")
	if address == "" {
		t.Skip("requires the Leads Core development database with migrations")
	}
	pool, err := pgxpool.New(t.Context(), address)
	if err != nil {
		t.Fatal("test database unavailable")
	}
	defer pool.Close()
	repo := NewRepository(pool)
	b := WebsiteInstallation{OrganizationID: newID("website_test"), WebsiteID: "web_" + strings.Repeat("f", 32), WebsiteRevision: 1, Site: "https://triodelab.no/", Environment: "development"}
	defer repo.DeleteByOrg(context.Background(), b.OrganizationID)
	v := WebsiteInquiry{SubmissionID: "95572209-f995-45a3-93b8-b6e8f115b156", Name: "Development test", Email: "test@example.com", Message: "An isolated receipt verification.", Source: "contact"}
	digest := bytes.Repeat([]byte{1}, 32)
	a, err := repo.ReceiveWebsiteInquiry(t.Context(), b, v, digest)
	if err != nil {
		t.Fatal(err)
	}
	retry, err := repo.ReceiveWebsiteInquiry(t.Context(), b, v, digest)
	if err != nil || retry != a {
		t.Fatal("retry did not return original receipt", err)
	}
	if _, err = repo.ReceiveWebsiteInquiry(t.Context(), b, v, bytes.Repeat([]byte{2}, 32)); err != ErrWebsiteConflict {
		t.Fatal("changed content overwrote submission")
	}
	zone, _ := time.LoadLocation("Europe/Oslo")
	day := time.Now().In(zone).Format("2006-01-02")
	report, err := repo.WebsiteOutcomes(t.Context(), b, day, day)
	if err != nil || report.Inquiries != 1 || report.Leads != 1 || report.Bookings != nil || report.LastReceivedAt == nil {
		t.Fatal("wrong confirmed outcomes", err)
	}
	for _, change := range []func(*WebsiteInstallation){func(x *WebsiteInstallation) { x.Environment = "production" }, func(x *WebsiteInstallation) { x.OrganizationID += "other" }, func(x *WebsiteInstallation) { x.WebsiteRevision++ }} {
		other := b
		change(&other)
		report, err = repo.WebsiteOutcomes(t.Context(), other, day, day)
		if err != nil || report.Inquiries != 0 {
			t.Fatal("cross-scope outcome escaped", err)
		}
	}
	if count, err := repo.DeleteByOrg(t.Context(), b.OrganizationID); err != nil || count != 1 {
		t.Fatal("erasure did not remove inquiry", err)
	}
	report, err = repo.WebsiteOutcomes(t.Context(), b, day, day)
	if err != nil || report.Inquiries != 0 || report.LastReceivedAt != nil {
		t.Fatal("erased inquiry still reported", err)
	}
}
