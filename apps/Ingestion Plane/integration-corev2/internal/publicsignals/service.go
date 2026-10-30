package publicsignals

import (
	"context"
	"errors"
	"io"
	"net"
	"net/http"
	"time"

	"github.com/triodelab/integration-corev2/internal/egress"
	"github.com/triodelab/integration-corev2/internal/pagespeed"
)

const maxResponse = 128 << 10
const maxSafe = 9007199254740991

type Service struct {
	key           string
	client        *http.Client
	now           func() time.Time
	gate          *requestGate
	active        chan struct{}
	indexSlot     chan struct{}
	latestIndex   string
	previousIndex string
	indexExpires  time.Time
}

type deviceResult struct {
	index  int
	device Device
}

func New(key string, client *http.Client, now func() time.Time) *Service {
	if client == nil {
		client = egress.SafeClient(egress.ClientConfig{RequestTimeout: 30 * time.Second})
	}
	copy := *client
	copy.Jar = nil
	copy.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	if copy.Timeout == 0 || copy.Timeout > 30*time.Second {
		copy.Timeout = 30 * time.Second
	}
	if now == nil {
		now = time.Now
	}
	return &Service{key: key, client: &copy, now: now, gate: commonCrawlGate, active: make(chan struct{}, 2), indexSlot: make(chan struct{}, 1)}
}

// Collect only queries fixed public-data endpoints. The selected website is
// query content, never an outbound destination. No ownership grant is created.
func (s *Service) Collect(parent context.Context, site string) (Report, error) {
	if pagespeed.ValidateInput(site, "mobile") != nil {
		return Report{}, ErrInvalid
	}
	if err := parent.Err(); err != nil {
		return Report{}, err
	}
	select {
	case s.active <- struct{}{}:
	default:
		return Report{}, ErrBusy
	}
	defer func() { <-s.active }()
	ctx, cancel := context.WithTimeout(parent, 30*time.Second)
	defer cancel()
	devices := []Device{unavailableDevice("PHONE", "source_unavailable"), unavailableDevice("DESKTOP", "source_unavailable")}
	cruxResults := make(chan deviceResult, 2)
	for index, device := range devices {
		go func(i int, factor string) { cruxResults <- deviceResult{i, s.crux(ctx, site, factor)} }(index, device.FormFactor)
	}
	archiveResults := make(chan CommonCrawl, 1)
	archiveIndexes := make(chan string, 1)
	go func() {
		archiveResults <- s.archiveWithIndex(ctx, site, func(index string) { archiveIndexes <- index })
	}()
	devices = collectDeviceResults(ctx, cruxResults, devices)
	archive := collectArchiveResult(ctx, archiveResults, archiveIndexes)
	return Report{Site: site, Source: "public_web_signals", ObservedAt: s.now().UTC().Format(time.RFC3339Nano), CrUX: devices, CommonCrawl: archive}, nil
}

func collectDeviceResults(ctx context.Context, results <-chan deviceResult, devices []Device) []Device {
	received := [2]bool{}
	for range 2 {
		select {
		case v := <-results:
			devices[v.index] = v.device
			received[v.index] = true
		case <-ctx.Done():
			for i, complete := range received {
				if !complete {
					devices[i] = unavailableDevice(devices[i].FormFactor, contextReason(ctx))
				}
			}
		}
	}
	// Cancellation can be ready at the same time as an already completed
	// source. Drain only buffered completions; never wait beyond the deadline.
	for range 2 {
		select {
		case v := <-results:
			devices[v.index] = v.device
		default:
		}
	}
	return devices
}

func collectArchiveResult(ctx context.Context, results <-chan CommonCrawl, indexes <-chan string) CommonCrawl {
	// A completed source must win over a different source consuming the overall
	// deadline. No worker join may extend that deadline.
	select {
	case result := <-results:
		return result
	default:
	}
	select {
	case result := <-results:
		return result
	case <-ctx.Done():
		select {
		case result := <-results:
			return result
		default:
		}
		result := unavailableArchive(contextReason(ctx))
		select {
		case index := <-indexes:
			result.Index = index
		default:
		}
		return result
	}
}

func contextReason(ctx context.Context) string {
	if errors.Is(ctx.Err(), context.DeadlineExceeded) {
		return "timeout"
	}
	return "source_unavailable"
}

func unavailableDevice(factor, reason string) Device {
	return Device{FormFactor: factor, Status: "unavailable", Reason: reason}
}
func unavailableArchive(reason string) CommonCrawl {
	return CommonCrawl{Status: "unavailable", Reason: reason, Captures: []Capture{}, Limit: 20}
}
func statusReason(status int) string {
	switch status {
	case 404:
		return "no_data"
	case 429, 503:
		return "quota_limited"
	case 504:
		return "timeout"
	}
	return "source_unavailable"
}

func (s *Service) request(req *http.Request, max int64) ([]byte, string) {
	raw, reason, _ := s.requestResponse(req, max)
	return raw, reason
}

func requestFailure(err error) string {
	var timeout net.Error
	if errors.Is(err, context.DeadlineExceeded) || errors.As(err, &timeout) && timeout.Timeout() {
		return "timeout"
	}
	return "source_unavailable"
}

func (s *Service) requestResponse(req *http.Request, max int64) ([]byte, string, int) {
	if req.URL.Scheme != "https" || req.URL.User != nil || req.URL.Port() != "" || req.URL.Fragment != "" || (req.URL.Host != "chromeuxreport.googleapis.com" && req.URL.Host != "index.commoncrawl.org") {
		return nil, "source_unavailable", 0
	}
	response, err := s.client.Do(req)
	if err != nil {
		return nil, requestFailure(err), 0
	}
	defer response.Body.Close()
	if response.StatusCode != 200 {
		if response.StatusCode == 403 && req.URL.Host == "chromeuxreport.googleapis.com" {
			return nil, "not_configured", response.StatusCode
		}
		return nil, statusReason(response.StatusCode), response.StatusCode
	}
	raw, err := io.ReadAll(io.LimitReader(response.Body, max+1))
	if err != nil {
		return nil, requestFailure(err), response.StatusCode
	}
	if int64(len(raw)) > max {
		return nil, "source_unavailable", response.StatusCode
	}
	return raw, "", response.StatusCode
}
