package nats

import (
	"time"

	"github.com/nats-io/nats.go"
)

type Config struct {
	URL         string
	User        string
	Password    string
	InboxPrefix string
	Name        string
}

type Client struct {
	Conn *nats.Conn
	JS   nats.JetStreamContext
}

func NewClient(cfg Config) (*Client, error) {
	options := []nats.Option{
		nats.Name(cfg.Name),
		nats.Timeout(5 * time.Second),
	}
	credential, err := selectRuntimeCredential(cfg.User, cfg.Password)
	if err != nil {
		return nil, err
	}
	options = append(options, nats.UserInfo(credential.User, credential.Password))
	if cfg.InboxPrefix != "" {
		options = append(options, nats.CustomInboxPrefix(cfg.InboxPrefix))
	}
	conn, err := nats.Connect(cfg.URL, options...)
	if err != nil {
		return nil, err
	}
	js, err := conn.JetStream()
	if err != nil {
		conn.Close()
		return nil, err
	}
	return &Client{Conn: conn, JS: js}, nil
}
