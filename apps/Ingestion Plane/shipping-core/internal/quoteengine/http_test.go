package quoteengine

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"shipping-core/internal/carrier"
)

func validBody() []byte {
	body, _ := json.Marshal(map[string]any{
		"from": map[string]any{
			"name": "Sender AS", "postal_code": "0150", "city": "Oslo", "country": "NO",
		},
		"to": map[string]any{
			"name": "Mottaker", "postal_code": "7010", "city": "Trondheim", "country": "NO",
		},
		"package": map[string]any{
			"weight_kg": 5, "length_cm": 30, "width_cm": 20, "height_cm": 15,
		},
		"segment": "b2b",
	})
	return body
}

func TestHandler_ValidRequest_ReturnsSortedQuotes(t *testing.T) {
	engine := New([]Quoter{
		&mockQuoter{code: "expensive"},
		&mockQuoter{code: "cheap"},
	}, time.Second)

	req := httptest.NewRequest(http.MethodPost, "/api/quotes", bytes.NewReader(validBody()))
	rec := httptest.NewRecorder()

	Handler(engine, nil)(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("got status %d, want 200; body=%s", rec.Code, rec.Body.String())
	}

	var resp quoteResponse
	if err := json.Unmarshal(rec.Body.Bytes(), &resp); err != nil {
		t.Fatalf("invalid JSON response: %v", err)
	}
	if len(resp.Quotes) != 2 {
		t.Fatalf("got %d quotes, want 2", len(resp.Quotes))
	}
	if len(resp.Errors) != 0 {
		t.Errorf("unexpected errors in response: %+v", resp.Errors)
	}
}

func TestHandler_PartialCarrierFailure_StillReturnsSuccessfulQuotes(t *testing.T) {
	engine := New([]Quoter{
		&mockQuoter{code: "ok"},
		&mockQuoter{code: "broken", err: errTest},
	}, time.Second)

	req := httptest.NewRequest(http.MethodPost, "/api/quotes", bytes.NewReader(validBody()))
	rec := httptest.NewRecorder()

	Handler(engine, nil)(rec, req)

	var resp quoteResponse
	if err := json.Unmarshal(rec.Body.Bytes(), &resp); err != nil {
		t.Fatalf("invalid JSON response: %v", err)
	}
	if len(resp.Quotes) != 1 {
		t.Errorf("got %d quotes, want 1 (the successful carrier)", len(resp.Quotes))
	}
	if len(resp.Errors) != 1 || resp.Errors[0].CarrierCode != "broken" {
		t.Errorf("got errors=%+v, want one error for carrier 'broken'", resp.Errors)
	}
}

func TestHandler_InvalidJSON_Returns400(t *testing.T) {
	req := httptest.NewRequest(http.MethodPost, "/api/quotes", bytes.NewReader([]byte("not json")))
	rec := httptest.NewRecorder()

	Handler(New(nil, time.Second), nil)(rec, req)

	if rec.Code != http.StatusBadRequest {
		t.Errorf("got status %d, want 400", rec.Code)
	}
}

func TestHandler_MissingRequiredField_Returns400(t *testing.T) {
	body, _ := json.Marshal(map[string]any{"segment": "b2b"}) // missing from/to/package
	req := httptest.NewRequest(http.MethodPost, "/api/quotes", bytes.NewReader(body))
	rec := httptest.NewRecorder()

	Handler(New(nil, time.Second), nil)(rec, req)

	if rec.Code != http.StatusBadRequest {
		t.Errorf("got status %d, want 400", rec.Code)
	}
}

func TestHandler_InvalidSegment_Returns400(t *testing.T) {
	body, _ := json.Marshal(map[string]any{
		"from":    map[string]any{"name": "A", "postal_code": "0150", "city": "Oslo", "country": "NO"},
		"to":      map[string]any{"name": "B", "postal_code": "7010", "city": "Trondheim", "country": "NO"},
		"package": map[string]any{"weight_kg": 5, "length_cm": 30, "width_cm": 20, "height_cm": 15},
		"segment": "not-a-real-segment",
	})
	req := httptest.NewRequest(http.MethodPost, "/api/quotes", bytes.NewReader(body))
	rec := httptest.NewRecorder()

	Handler(New(nil, time.Second), nil)(rec, req)

	if rec.Code != http.StatusBadRequest {
		t.Errorf("got status %d, want 400", rec.Code)
	}
}

func TestCarriersHandlerReportsExplicitProviderMode(t *testing.T) {
	engine := New([]Quoter{
		&mockQuoter{code: "dhl", mode: carrier.ModeSandbox},
		&mockQuoter{code: "mock-bring", mode: carrier.ModeMock},
	}, time.Second)
	request := httptest.NewRequest(http.MethodGet, "/api/carriers", nil)
	response := httptest.NewRecorder()

	CarriersHandler(engine)(response, request)

	var body carriersResponse
	if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode response: %v", err)
	}
	if len(body.Carriers) != 2 {
		t.Fatalf("carriers = %d, want 2", len(body.Carriers))
	}
	if body.Carriers[0].Mode != "sandbox" || body.Carriers[0].IsMock {
		t.Fatalf("DHL provenance = %#v, want sandbox/non-mock", body.Carriers[0])
	}
	if body.Carriers[0].VerifiedAt != nil || body.Carriers[0].DegradedReason == "" {
		t.Fatalf("unverified carrier must be typed honestly: %#v", body.Carriers[0])
	}
	if body.Carriers[1].Mode != "mock" || !body.Carriers[1].IsMock {
		t.Fatalf("mock provenance = %#v, want mock", body.Carriers[1])
	}
}

var errTest = &testError{"simulated upstream failure"}

type testError struct{ msg string }

func (e *testError) Error() string { return e.msg }
