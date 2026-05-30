package nats

import (
	"time"

	"github.com/nats-io/nats.go"
)

type Config struct {
	URL   string
	Token string
	Name  string
}

func NewClient(cfg Config) (*nats.Conn, error) {
	options := []nats.Option{
		nats.Name(cfg.Name),
		nats.Timeout(5 * time.Second),
	}
	if cfg.Token != "" {
		options = append(options, nats.Token(cfg.Token))
	}
	return nats.Connect(cfg.URL, options...)
}
