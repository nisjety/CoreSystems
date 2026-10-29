package nats

import (
	"fmt"
	"strings"
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
	user := strings.TrimSpace(cfg.User)
	password := strings.TrimSpace(cfg.Password)
	if user == "" || len(password) < 32 {
		return nil, fmt.Errorf("scoped NATS user and password of at least 32 characters are required")
	}
	options := []nats.Option{
		nats.Name(cfg.Name),
		nats.Timeout(5 * time.Second),
		nats.UserInfo(user, password),
	}
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

func (c *Client) Close() {
	if c == nil || c.Conn == nil {
		return
	}
	c.Conn.Close()
}
