// Package algoliamcp is Integration's bounded, aggregate-only MCP consumer.
package algoliamcp

import (
	"context"
	"errors"
	"github.com/triodelab/integration-corev2/internal/oauth"
	"github.com/triodelab/integration-corev2/internal/store"
	"regexp"
)

var ErrUnavailable = errors.New("Algolia read-only reports are unavailable")
var ErrAccess = errors.New("Algolia connection is not available in this organization")
var ErrBusy = errors.New("Algolia reporting capacity is busy")
var appPattern = regexp.MustCompile(`^[A-Z0-9]{8,32}$`)
var indexPattern = regexp.MustCompile(`^[A-Za-z0-9_.-]{1,128}$`)
var connectionPattern = regexp.MustCompile(`^conn_[A-Za-z0-9_-]{1,128}$`)

type Repository interface {
	GetConnection(context.Context, string) (store.Connection, error)
}
type Tokens interface {
	AlgoliaAccessToken(context.Context, string, string) (oauth.AccessTokenResult, error)
}
type Application struct {
	ID string `json:"id"`
}
type Index struct {
	Name string `json:"name"`
}
type Access struct {
	ConnectionID string `json:"connectionId"`
	Status       string `json:"status"`
	Generation   string `json:"generation"`
}
type ResourceInput struct {
	ConnectionID  string `json:"connectionId"`
	ApplicationID string `json:"applicationId"`
	IndexName     string `json:"indexName"`
}
type Resources struct {
	ConnectionID  string `json:"connectionId"`
	ApplicationID string `json:"applicationId"`
	IndexName     string `json:"indexName"`
	Status        string `json:"status"`
}
type ReportInput struct {
	WebsiteID       string `json:"websiteId"`
	Site            string `json:"site"`
	WebsiteRevision int64  `json:"websiteRevision"`
	ConnectionID    string `json:"connectionId"`
	ApplicationID   string `json:"applicationId"`
	IndexName       string `json:"indexName"`
}
