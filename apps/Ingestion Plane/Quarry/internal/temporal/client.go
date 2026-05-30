package temporal

import (
	"fmt"

	"go.temporal.io/sdk/client"

	"github.com/triodelab/quarry/internal/config"
)

func NewClient(cfg *config.Config) (client.Client, error) {
	if cfg == nil {
		return nil, fmt.Errorf("config is nil")
	}

	return client.Dial(client.Options{
		HostPort:  cfg.TemporalAddress,
		Namespace: cfg.TemporalNamespace,
	})
}
