package recommend

import (
	"context"
	"strings"
	"testing"
	"time"

	"shipping-core/internal/carrier"
	"shipping-core/internal/modelplane"
)

type fakeModelClient struct {
	configured bool
	response   modelplane.InvokeResponse
	err        error
	gotPrompt  string
}

func (f *fakeModelClient) Configured() bool { return f.configured }

func (f *fakeModelClient) Invoke(_ context.Context, req modelplane.InvokeRequest) (modelplane.InvokeResponse, error) {
	f.gotPrompt = req.Content
	return f.response, f.err
}

func testQuotes() []carrier.Quote {
	return []carrier.Quote{
		{CarrierCode: "mock-bring", CarrierName: "Bring", ServiceName: "Bring Standard", Price: carrier.Money{AmountCents: 13300, Currency: "NOK"}, TransitDays: 3},
		{CarrierCode: "ups", CarrierName: "UPS", ServiceName: "UPS Express", Price: carrier.Money{AmountCents: 45000, Currency: "NOK"}, TransitDays: 1},
	}
}

func testReq() carrier.QuoteRequest {
	return carrier.QuoteRequest{
		From:    carrier.Address{City: "Oslo", Country: "NO"},
		To:      carrier.Address{City: "Trondheim", Country: "NO"},
		Package: carrier.Package{WeightKg: 5, LengthCm: 30, WidthCm: 20, HeightCm: 15},
		Segment: carrier.SegmentB2B,
	}
}

func TestRecommend_NotConfigured_ReturnsHonestUnavailable(t *testing.T) {
	client := &fakeModelClient{configured: false}
	rec := Recommend(context.Background(), client, testReq(), testQuotes())
	if rec.Available {
		t.Fatal("expected Available=false when client is not configured")
	}
	if rec.UnavailableReason == "" {
		t.Error("expected a non-empty UnavailableReason")
	}
}

func TestRecommend_NoQuotes_ReturnsHonestUnavailable(t *testing.T) {
	client := &fakeModelClient{configured: true}
	rec := Recommend(context.Background(), client, testReq(), nil)
	if rec.Available {
		t.Fatal("expected Available=false with no quotes")
	}
}

func TestRecommend_ValidResponse_ParsesAndValidatesCarrierCode(t *testing.T) {
	client := &fakeModelClient{
		configured: true,
		response: modelplane.InvokeResponse{
			Content:   `{"recommended_carrier_code":"mock-bring","recommended_service_name":"Bring Standard","reasoning":"Cheapest with acceptable transit time.","confidence":0.8,"tradeoffs":["slower than UPS"]}`,
			ModelUsed: "verevon-balance",
		},
	}
	rec := Recommend(context.Background(), client, testReq(), testQuotes())
	if !rec.Available {
		t.Fatalf("expected Available=true, got UnavailableReason=%q", rec.UnavailableReason)
	}
	if rec.RecommendedCarrierCode != "mock-bring" {
		t.Errorf("RecommendedCarrierCode = %q", rec.RecommendedCarrierCode)
	}
	if rec.Confidence != 0.8 {
		t.Errorf("Confidence = %v, want 0.8", rec.Confidence)
	}
	if len(rec.Tradeoffs) != 1 {
		t.Errorf("Tradeoffs = %+v", rec.Tradeoffs)
	}
	if client.gotPrompt == "" || !strings.Contains(client.gotPrompt, "mock-bring") {
		t.Errorf("prompt should list carrier_code=mock-bring, got: %s", client.gotPrompt)
	}
}

func TestRecommend_HallucinatedCarrierCode_RejectedHonestly(t *testing.T) {
	client := &fakeModelClient{
		configured: true,
		response: modelplane.InvokeResponse{
			Content: `{"recommended_carrier_code":"dhl-express-nonexistent","reasoning":"x","confidence":0.9}`,
		},
	}
	rec := Recommend(context.Background(), client, testReq(), testQuotes())
	if rec.Available {
		t.Fatal("expected Available=false for a carrier_code that wasn't quoted")
	}
}

func TestRecommend_PreservesSelectedQuoteProvenance(t *testing.T) {
	stamp := time.Date(2026, 9, 19, 12, 0, 0, 0, time.UTC)
	quotes := testQuotes()
	quotes[0].Environment = "mock"
	quotes[0].IsMock = true
	quotes[0].QuotedAt = stamp
	quotes[0].PackageCount = 1
	client := &fakeModelClient{configured: true, response: modelplane.InvokeResponse{
		Content: `{"recommended_carrier_code":"mock-bring","reasoning":"Test quote only.","confidence":0.8}`,
	}}
	rec := Recommend(context.Background(), client, testReq(), quotes)
	if !rec.Available || rec.Environment != "mock" || !rec.IsMock || !rec.QuotedAt.Equal(stamp) || rec.PackageCount != 1 || rec.RecommendedServiceName != "Bring Standard" {
		t.Fatalf("quote provenance was lost: %+v", rec)
	}
	if !strings.Contains(client.gotPrompt, "environment=mock, package_count=1, quoted_at=2026-09-19T12:00:00Z") {
		t.Fatal("model did not receive quote provenance")
	}
}

func TestRecommend_UnquotedServiceRejected(t *testing.T) {
	client := &fakeModelClient{configured: true, response: modelplane.InvokeResponse{
		Content: `{"recommended_carrier_code":"ups","recommended_service_name":"Invented overnight","reasoning":"Fast.","confidence":0.8}`,
	}}
	if rec := Recommend(context.Background(), client, testReq(), testQuotes()); rec.Available {
		t.Fatalf("unquoted service was recommended: %+v", rec)
	}
}

func TestRecommend_MalformedJSON_ReturnsHonestUnavailable(t *testing.T) {
	client := &fakeModelClient{configured: true, response: modelplane.InvokeResponse{Content: "not json at all"}}
	rec := Recommend(context.Background(), client, testReq(), testQuotes())
	if rec.Available {
		t.Fatal("expected Available=false for malformed model output")
	}
}

func TestRecommend_ModelPlaneError_ReturnsHonestUnavailable(t *testing.T) {
	client := &fakeModelClient{configured: true, err: errTest("model-gateway unreachable")}
	rec := Recommend(context.Background(), client, testReq(), testQuotes())
	if rec.Available {
		t.Fatal("expected Available=false when the model plane call errors")
	}
}

type errTest string

func (e errTest) Error() string { return string(e) }
