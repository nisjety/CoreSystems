package ups

import "testing"

func TestNewConfigFromEnv(t *testing.T) {
	tests := []struct {
		name    string
		env     map[string]string
		wantErr bool
	}{
		{
			name: "required vars set",
			env: map[string]string{
				"UPS_CLIENT_ID":     "id",
				"UPS_CLIENT_SECRET": "secret",
			},
			wantErr: false,
		},
		{name: "all missing", env: map[string]string{}, wantErr: true},
		{
			name:    "missing secret",
			env:     map[string]string{"UPS_CLIENT_ID": "id"},
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			for _, key := range []string{"UPS_CLIENT_ID", "UPS_CLIENT_SECRET", "UPS_ACCOUNT_NUMBER", "UPS_API_BASE_URL"} {
				t.Setenv(key, tt.env[key])
			}
			_, err := NewConfigFromEnv()
			if (err != nil) != tt.wantErr {
				t.Fatalf("got err=%v, wantErr=%v", err, tt.wantErr)
			}
		})
	}
}

func TestNewConfigFromEnv_OptionalFields(t *testing.T) {
	t.Setenv("UPS_CLIENT_ID", "id")
	t.Setenv("UPS_CLIENT_SECRET", "secret")
	t.Setenv("UPS_ACCOUNT_NUMBER", "A1B2C3")
	t.Setenv("UPS_API_BASE_URL", "https://wwwcie.ups.com")

	cfg, err := NewConfigFromEnv()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if cfg.AccountNumber != "A1B2C3" || cfg.BaseURL != "https://wwwcie.ups.com" {
		t.Errorf("optional fields not read: %+v", cfg)
	}
}
