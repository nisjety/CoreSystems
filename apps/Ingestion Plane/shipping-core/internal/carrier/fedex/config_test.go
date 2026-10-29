package fedex

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
				"FEDEX_CLIENT_ID":      "id",
				"FEDEX_CLIENT_SECRET":  "secret",
				"FEDEX_ACCOUNT_NUMBER": "740561073",
			},
			wantErr: false,
		},
		{name: "all missing", env: map[string]string{}, wantErr: true},
		{
			name: "missing account number",
			env: map[string]string{
				"FEDEX_CLIENT_ID":     "id",
				"FEDEX_CLIENT_SECRET": "secret",
			},
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			for _, key := range []string{"FEDEX_CLIENT_ID", "FEDEX_CLIENT_SECRET", "FEDEX_ACCOUNT_NUMBER", "FEDEX_API_BASE_URL"} {
				t.Setenv(key, tt.env[key])
			}
			_, err := NewConfigFromEnv()
			if (err != nil) != tt.wantErr {
				t.Fatalf("got err=%v, wantErr=%v", err, tt.wantErr)
			}
		})
	}
}

func TestNewConfigFromEnv_SandboxBaseURL(t *testing.T) {
	t.Setenv("FEDEX_CLIENT_ID", "id")
	t.Setenv("FEDEX_CLIENT_SECRET", "secret")
	t.Setenv("FEDEX_ACCOUNT_NUMBER", "740561073")
	t.Setenv("FEDEX_API_BASE_URL", "https://apis-sandbox.fedex.com")

	cfg, err := NewConfigFromEnv()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if cfg.BaseURL != "https://apis-sandbox.fedex.com" {
		t.Errorf("BaseURL = %q, want sandbox override", cfg.BaseURL)
	}
}
