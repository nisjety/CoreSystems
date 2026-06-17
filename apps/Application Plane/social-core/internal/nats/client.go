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

type Client struct {
	Conn *nats.Conn
	JS   nats.JetStreamContext
}

func NewClient(cfg Config) (*Client, error) {
	options := []nats.Option{
		nats.Name(cfg.Name),
		nats.Timeout(5 * time.Second),
	}
	if cfg.Token != "" {
		options = append(options, nats.Token(cfg.Token))
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

func (c *Client) Close() {
	if c == nil || c.Conn == nil {
		return
	}
	c.Conn.Close()
}
