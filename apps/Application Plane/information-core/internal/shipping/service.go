package shipping

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"strings"

	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/config"
)

const defaultTrackingURL = "https://tracking.bring.com/tracking.json"

type Service struct {
	client     *http.Client
	cache      *cache.Store
	trackingURL string
	apiUID     string
	apiKey     string
}

type ShipmentStatus struct {
	TrackingNumber    string  `json:"trackingNumber"`
	Status            string  `json:"status"`
	StatusCode        string  `json:"statusCode"`
	Description       string  `json:"description"`
	Carrier           string  `json:"carrier"`
	EstimatedDelivery string  `json:"estimatedDelivery,omitempty"`
	LastUpdate        string  `json:"lastUpdate,omitempty"`
	Events            []Event `json:"events"`
}

type Event struct {
	Timestamp   string `json:"timestamp"`
	Description string `json:"description"`
	Location    string `json:"location,omitempty"`
}

// bringResponse maps the Bring Tracking API v2 JSON envelope.
// Spec: https://developer.bring.com/api/tracking/
type bringResponse struct {
	ConsignmentSet []struct {
		ConsignmentID string `json:"consignmentId"`
		Error         *struct {
			Code    string `json:"code"`
			Message string `json:"message"`
		} `json:"error"`
		PackageSet []struct {
			StatusDescription       string `json:"statusDescription"`
			StatusCode              string `json:"statusCode"`
			DateOfEstimatedDelivery string `json:"dateOfEstimatedDelivery"`
			EventSet                []struct {
				Description       string `json:"description"`
				OccurrenceDatestamp string `json:"occurrenceDatestamp"`
				LimitedAddress    *struct {
					City    string `json:"city"`
					Country string `json:"country"`
				} `json:"limitedAddress"`
			} `json:"eventSet"`
		} `json:"packageSet"`
	} `json:"consignmentSet"`
}

func NewService(client *http.Client, cacheStore *cache.Store, apiUID, apiKey string) *Service {
	return &Service{
		client:      client,
		cache:       cacheStore,
		trackingURL: defaultTrackingURL,
		apiUID:      apiUID,
		apiKey:      apiKey,
	}
}

// NewServiceWithURL is used in tests to point at a local httptest server.
func NewServiceWithURL(client *http.Client, cacheStore *cache.Store, apiUID, apiKey, trackingURL string) *Service {
	svc := NewService(client, cacheStore, apiUID, apiKey)
	svc.trackingURL = trackingURL
	return svc
}

func (s *Service) Track(ctx context.Context, trackingNumber string) (ShipmentStatus, error) {
	trackingNumber = strings.TrimSpace(strings.ToUpper(trackingNumber))
	if trackingNumber == "" {
		return ShipmentStatus{}, fmt.Errorf("shipping: trackingNumber is required")
	}

	key := "shipping:" + trackingNumber
	if cached, ok := s.cache.Get(key); ok {
		if payload, ok := cached.(ShipmentStatus); ok {
			return payload, nil
		}
	}

	params := url.Values{}
	params.Set("q", trackingNumber)

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, s.trackingURL+"?"+params.Encode(), nil)
	if err != nil {
		return ShipmentStatus{}, fmt.Errorf("shipping: build request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	if s.apiUID != "" {
		req.Header.Set("X-MyBring-API-Uid", s.apiUID)
		req.Header.Set("X-MyBring-API-Key", s.apiKey)
	}

	resp, err := s.client.Do(req)
	if err != nil {
		return ShipmentStatus{}, fmt.Errorf("shipping: upstream: %w", err)
	}
	defer resp.Body.Close()

	var raw bringResponse
	if err := json.NewDecoder(resp.Body).Decode(&raw); err != nil {
		return ShipmentStatus{}, fmt.Errorf("shipping: decode: %w", err)
	}
	if len(raw.ConsignmentSet) == 0 {
		return ShipmentStatus{}, fmt.Errorf("shipping: no data for %s", trackingNumber)
	}

	c := raw.ConsignmentSet[0]
	if c.Error != nil {
		return ShipmentStatus{}, fmt.Errorf("shipping: %s: %s", c.Error.Code, c.Error.Message)
	}
	if len(c.PackageSet) == 0 {
		return ShipmentStatus{TrackingNumber: trackingNumber, Status: "Unknown", StatusCode: "UNKNOWN", Carrier: "Bring/Posten"}, nil
	}

	pkg := c.PackageSet[0]
	events := make([]Event, 0, len(pkg.EventSet))
	for _, e := range pkg.EventSet {
		loc := ""
		if e.LimitedAddress != nil {
			loc = strings.TrimSpace(e.LimitedAddress.City + ", " + e.LimitedAddress.Country)
			loc = strings.Trim(loc, ", ")
		}
		events = append(events, Event{
			Timestamp:   e.OccurrenceDatestamp,
			Description: e.Description,
			Location:    loc,
		})
	}

	lastUpdate := ""
	if len(events) > 0 {
		lastUpdate = events[0].Timestamp
	}

	result := ShipmentStatus{
		TrackingNumber:    trackingNumber,
		Status:            normalizeStatus(pkg.StatusCode),
		StatusCode:        pkg.StatusCode,
		Description:       pkg.StatusDescription,
		Carrier:           "Bring/Posten",
		EstimatedDelivery: pkg.DateOfEstimatedDelivery,
		LastUpdate:        lastUpdate,
		Events:            events,
	}

	s.cache.Set(key, config.TTL(600), result)
	return result, nil
}

func normalizeStatus(code string) string {
	switch strings.ToUpper(code) {
	case "DELIVERED":
		return "Delivered"
	case "IN_TRANSIT":
		return "In transit"
	case "ATTEMPTED_DELIVERY":
		return "Delivery attempted"
	case "CUSTOMS":
		return "In customs"
	case "NOTIFICATION_SENT":
		return "Notification sent"
	case "PRE_NOTIFIED":
		return "Pre-notified"
	case "READY_FOR_PICKUP":
		return "Ready for pickup"
	case "RETURNED":
		return "Returned"
	case "EXCEPTION":
		return "Exception"
	default:
		return "In progress"
	}
}
