package dhl

import "testing"

func TestNewConfigFromEnv(t *testing.T) {
	tests := []struct {
		name    string
		env     map[string]string
		wantErr bool
	}{
		{
			name: "all required vars set",
			env: map[string]string{
				"DHL_API_KEY":    "key",
				"DHL_API_SECRET": "secret",
			},
			wantErr: false,
		},
		{
			name:    "all missing",
			env:     map[string]string{},
			wantErr: true,
		},
		{
			name: "missing secret",
			env: map[string]string{
				"DHL_API_KEY": "key",
			},
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			for _, key := range []string{"DHL_API_KEY", "DHL_API_SECRET", "DHL_ACCOUNT_NUMBER", "DHL_API_BASE_URL"} {
				t.Setenv(key, tt.env[key])
			}

			cfg, err := NewConfigFromEnv()
			if (err != nil) != tt.wantErr {
				t.Fatalf("got err=%v, wantErr=%v", err, tt.wantErr)
			}
			if !tt.wantErr && cfg.APIKey != tt.env["DHL_API_KEY"] {
				t.Errorf("cfg.APIKey = %q, want %q", cfg.APIKey, tt.env["DHL_API_KEY"])
			}
		})
	}
}

func TestNewConfigFromEnv_CustomBaseURL(t *testing.T) {
	t.Setenv("DHL_API_KEY", "key")
	t.Setenv("DHL_API_SECRET", "secret")
	t.Setenv("DHL_API_BASE_URL", "https://express.api.dhl.com/mydhlapi/test")

	cfg, err := NewConfigFromEnv()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if cfg.BaseURL != "https://express.api.dhl.com/mydhlapi/test" {
		t.Errorf("cfg.BaseURL = %q, want sandbox override", cfg.BaseURL)
	}
}
