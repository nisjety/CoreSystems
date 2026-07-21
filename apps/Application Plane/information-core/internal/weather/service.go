package weather

import (
	"context"
	"encoding/json"
	"fmt"
	"math"
	"net/http"
	"net/url"
	"strings"
	"time"

	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/config"
	"coresystem/apps/application-plane/information-core/internal/provenance"
)

const (
	forecastURL = "https://api.met.no/weatherapi/locationforecast/2.0/compact"
	geocodeURL  = "https://nominatim.openstreetmap.org/reverse"
)

type Service struct {
	client *http.Client
	cache  *cache.Store
}

type Forecast struct {
	Source   provenance.Source `json:"source"`
	Current  Current           `json:"current"`
	Forecast []DailyForecast   `json:"forecast"`
}

type Current struct {
	Temperature   int     `json:"temperature"`
	Humidity      int     `json:"humidity"`
	WindSpeed     int     `json:"windSpeed"`
	WindDirection int     `json:"windDirection"`
	Precipitation float64 `json:"precipitation"`
	Pressure      int     `json:"pressure"`
	Condition     string  `json:"condition"`
	Icon          string  `json:"icon"`
	Location      string  `json:"location"`
	LastUpdated   string  `json:"lastUpdated"`
}

type DailyForecast struct {
	Date        string `json:"date"`
	Temperature struct {
		Min int `json:"min"`
		Max int `json:"max"`
	} `json:"temperature"`
	Condition     string  `json:"condition"`
	Icon          string  `json:"icon"`
	Precipitation float64 `json:"precipitation"`
}

type metResponse struct {
	Properties struct {
		TimeSeries []struct {
			Time string `json:"time"`
			Data struct {
				Instant struct {
					Details struct {
						AirTemperature    float64 `json:"air_temperature"`
						RelativeHumidity  float64 `json:"relative_humidity"`
						WindSpeed         float64 `json:"wind_speed"`
						WindFromDirection float64 `json:"wind_from_direction"`
						AirPressureSea    float64 `json:"air_pressure_at_sea_level"`
					} `json:"details"`
				} `json:"instant"`
				Next1Hours *struct {
					Summary struct {
						SymbolCode string `json:"symbol_code"`
					} `json:"summary"`
					Details struct {
						PrecipitationAmount float64 `json:"precipitation_amount"`
					} `json:"details"`
				} `json:"next_1_hours"`
				Next6Hours *struct {
					Summary struct {
						SymbolCode string `json:"symbol_code"`
					} `json:"summary"`
					Details struct {
						PrecipitationAmount float64 `json:"precipitation_amount"`
					} `json:"details"`
				} `json:"next_6_hours"`
			} `json:"data"`
		} `json:"timeseries"`
	} `json:"properties"`
}

func NewService(client *http.Client, cacheStore *cache.Store) *Service {
	return &Service{client: client, cache: cacheStore}
}

func (s *Service) Oslo(ctx context.Context, userAgent string) (Forecast, error) {
	return s.Forecast(ctx, userAgent, 59.9139, 10.7522, 90)
}

func (s *Service) Forecast(ctx context.Context, userAgent string, lat, lon float64, altitude int) (Forecast, error) {
	key := fmt.Sprintf("weather:%0.4f:%0.4f:%d", lat, lon, altitude)
	if cached, ok := s.cache.Get(key); ok {
		if payload, ok := cached.(Forecast); ok {
			return payload, nil
		}
	}

	location := s.locationName(ctx, userAgent, lat, lon)

	params := url.Values{}
	params.Set("lat", fmt.Sprintf("%.4f", lat))
	params.Set("lon", fmt.Sprintf("%.4f", lon))
	if altitude > 0 {
		params.Set("altitude", fmt.Sprintf("%d", altitude))
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, forecastURL+"?"+params.Encode(), nil)
	if err != nil {
		return Forecast{}, err
	}
	req.Header.Set("User-Agent", userAgent)

	resp, err := s.client.Do(req)
	if err != nil {
		return Forecast{}, err
	}
	defer resp.Body.Close()

	var payload metResponse
	if err := json.NewDecoder(resp.Body).Decode(&payload); err != nil {
		return Forecast{}, err
	}
	if len(payload.Properties.TimeSeries) == 0 {
		return Forecast{}, fmt.Errorf("weather upstream returned no timeseries")
	}

	currentEntry := payload.Properties.TimeSeries[0]
	icon := "cloudy"
	precipitation := 0.0
	if currentEntry.Data.Next1Hours != nil {
		icon = currentEntry.Data.Next1Hours.Summary.SymbolCode
		precipitation = currentEntry.Data.Next1Hours.Details.PrecipitationAmount
	}

	result := Forecast{
		Source: provenance.Source{
			Provider: "met.no", Dataset: "locationforecast-2.0-compact",
			SourceURL: forecastURL, License: "NLOD-2.0", RetrievedAt: time.Now().UTC().Format(time.RFC3339),
			Quality: "authoritative_provider", Coverage: "point_forecast", Status: "forecast", APIVersion: "2.0",
		},
		Current: Current{
			Temperature:   int(math.Round(currentEntry.Data.Instant.Details.AirTemperature)),
			Humidity:      int(math.Round(currentEntry.Data.Instant.Details.RelativeHumidity)),
			WindSpeed:     int(math.Round(currentEntry.Data.Instant.Details.WindSpeed)),
			WindDirection: int(math.Round(currentEntry.Data.Instant.Details.WindFromDirection)),
			Precipitation: round1(precipitation),
			Pressure:      int(math.Round(currentEntry.Data.Instant.Details.AirPressureSea)),
			Condition:     mapCondition(icon),
			Icon:          icon,
			Location:      location,
			LastUpdated:   currentEntry.Time,
		},
		Forecast: aggregateForecast(payload.Properties.TimeSeries),
	}

	s.cache.Set(key, config.TTL(600), result)
	return result, nil
}

func (s *Service) locationName(ctx context.Context, userAgent string, lat, lon float64) string {
	key := fmt.Sprintf("location:%0.4f:%0.4f", lat, lon)
	if cached, ok := s.cache.Get(key); ok {
		if value, ok := cached.(string); ok {
			return value
		}
	}

	params := url.Values{}
	params.Set("lat", fmt.Sprintf("%.4f", lat))
	params.Set("lon", fmt.Sprintf("%.4f", lon))
	params.Set("format", "json")
	params.Set("addressdetails", "1")
	params.Set("zoom", "10")

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, geocodeURL+"?"+params.Encode(), nil)
	if err != nil {
		return "Oslo"
	}
	req.Header.Set("User-Agent", userAgent)

	resp, err := s.client.Do(req)
	if err != nil {
		return "Oslo"
	}
	defer resp.Body.Close()

	var payload struct {
		DisplayName string `json:"display_name"`
		Address     struct {
			City         string `json:"city"`
			Town         string `json:"town"`
			Municipality string `json:"municipality"`
			County       string `json:"county"`
		} `json:"address"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&payload); err != nil {
		return "Oslo"
	}

	location := firstNonEmpty(payload.Address.City, payload.Address.Town, payload.Address.Municipality, payload.Address.County)
	if location == "" {
		location = firstSegment(payload.DisplayName)
	}
	if location == "" {
		location = "Oslo"
	}
	s.cache.Set(key, config.TTL(86400), location)
	return location
}

func aggregateForecast(entries []struct {
	Time string `json:"time"`
	Data struct {
		Instant struct {
			Details struct {
				AirTemperature    float64 `json:"air_temperature"`
				RelativeHumidity  float64 `json:"relative_humidity"`
				WindSpeed         float64 `json:"wind_speed"`
				WindFromDirection float64 `json:"wind_from_direction"`
				AirPressureSea    float64 `json:"air_pressure_at_sea_level"`
			} `json:"details"`
		} `json:"instant"`
		Next1Hours *struct {
			Summary struct {
				SymbolCode string `json:"symbol_code"`
			} `json:"summary"`
			Details struct {
				PrecipitationAmount float64 `json:"precipitation_amount"`
			} `json:"details"`
		} `json:"next_1_hours"`
		Next6Hours *struct {
			Summary struct {
				SymbolCode string `json:"symbol_code"`
			} `json:"summary"`
			Details struct {
				PrecipitationAmount float64 `json:"precipitation_amount"`
			} `json:"details"`
		} `json:"next_6_hours"`
	} `json:"data"`
}) []DailyForecast {
	type bucket struct {
		temps []float64
		icon  string
		rain  float64
	}
	days := make(map[string]*bucket)
	order := make([]string, 0, 7)
	for _, entry := range entries {
		day := strings.Split(entry.Time, "T")[0]
		b, ok := days[day]
		if !ok {
			b = &bucket{}
			days[day] = b
			order = append(order, day)
		}
		b.temps = append(b.temps, entry.Data.Instant.Details.AirTemperature)
		if b.icon == "" {
			switch {
			case entry.Data.Next6Hours != nil:
				b.icon = entry.Data.Next6Hours.Summary.SymbolCode
				b.rain = max(b.rain, entry.Data.Next6Hours.Details.PrecipitationAmount)
			case entry.Data.Next1Hours != nil:
				b.icon = entry.Data.Next1Hours.Summary.SymbolCode
				b.rain = max(b.rain, entry.Data.Next1Hours.Details.PrecipitationAmount)
			}
		}
	}

	result := make([]DailyForecast, 0, min(5, len(order)))
	for _, day := range order[:min(5, len(order))] {
		b := days[day]
		minTemp := math.MaxFloat64
		maxTemp := -math.MaxFloat64
		for _, temp := range b.temps {
			minTemp = minF(minTemp, temp)
			maxTemp = maxF(maxTemp, temp)
		}
		item := DailyForecast{
			Date:          day,
			Condition:     mapCondition(b.icon),
			Icon:          b.icon,
			Precipitation: round1(b.rain),
		}
		item.Temperature.Min = int(math.Round(minTemp))
		item.Temperature.Max = int(math.Round(maxTemp))
		result = append(result, item)
	}
	return result
}

func mapCondition(symbol string) string {
	symbol = strings.ToLower(symbol)
	switch {
	case strings.Contains(symbol, "clearsky"):
		return "Clear sky"
	case strings.Contains(symbol, "partlycloudy"):
		return "Partly cloudy"
	case strings.Contains(symbol, "cloudy"):
		return "Cloudy"
	case strings.Contains(symbol, "rain"):
		return "Rain"
	case strings.Contains(symbol, "snow"):
		return "Snow"
	case strings.Contains(symbol, "fog"):
		return "Fog"
	case strings.Contains(symbol, "thunder"):
		return "Thunder"
	default:
		return "Weather update"
	}
}

func round1(value float64) float64 {
	return math.Round(value*10) / 10
}

func firstSegment(input string) string {
	if idx := strings.Index(input, ","); idx > 0 {
		return strings.TrimSpace(input[:idx])
	}
	return strings.TrimSpace(input)
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if strings.TrimSpace(value) != "" {
			return strings.TrimSpace(value)
		}
	}
	return ""
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}

func minF(a, b float64) float64 {
	if a < b {
		return a
	}
	return b
}

func maxF(a, b float64) float64 {
	if a > b {
		return a
	}
	return b
}

func max(a, b float64) float64 {
	if a > b {
		return a
	}
	return b
}
