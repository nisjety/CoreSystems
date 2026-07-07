package bring

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
				"BRING_API_UID":         "user@example.com",
				"BRING_API_KEY":         "key",
				"BRING_CUSTOMER_NUMBER": "5",
			},
			wantErr: false,
		},
		{
			name:    "all missing",
			env:     map[string]string{},
			wantErr: true,
		},
		{
			name: "missing api key",
			env: map[string]string{
				"BRING_API_UID":         "user@example.com",
				"BRING_CUSTOMER_NUMBER": "5",
			},
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			for _, key := range []string{"BRING_API_UID", "BRING_API_KEY", "BRING_CUSTOMER_NUMBER", "BRING_API_BASE_URL"} {
				t.Setenv(key, tt.env[key])
			}

			cfg, err := NewConfigFromEnv()
			if (err != nil) != tt.wantErr {
				t.Fatalf("got err=%v, wantErr=%v", err, tt.wantErr)
			}
			if !tt.wantErr && cfg.APIUID != tt.env["BRING_API_UID"] {
				t.Errorf("cfg.APIUID = %q, want %q", cfg.APIUID, tt.env["BRING_API_UID"])
			}
		})
	}
}

func TestNewConfigFromEnv_CustomBaseURL(t *testing.T) {
	t.Setenv("BRING_API_UID", "user@example.com")
	t.Setenv("BRING_API_KEY", "key")
	t.Setenv("BRING_CUSTOMER_NUMBER", "5")
	t.Setenv("BRING_API_BASE_URL", "https://example.test/override")

	cfg, err := NewConfigFromEnv()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if cfg.BaseURL != "https://example.test/override" {
		t.Errorf("cfg.BaseURL = %q, want override", cfg.BaseURL)
	}
}
