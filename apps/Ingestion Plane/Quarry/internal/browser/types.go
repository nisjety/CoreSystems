package browser

import (
	"context"

	"github.com/triodelab/quarry/internal/session"
)

type CreateRequest struct {
	URL      string                  `json:"url"`
	Viewport *session.ViewportConfig `json:"viewport,omitempty"`
	Mobile   bool                    `json:"mobile,omitempty"`
	Profile  string                  `json:"profile,omitempty"`
	OrgID    string                  `json:"org_id,omitempty"`
	UserID   string                  `json:"user_id,omitempty"`
}

type SessionState struct {
	Session    session.SessionInfo `json:"session"`
	CurrentURL string              `json:"current_url,omitempty"`
}

type CreateResponse struct {
	Success bool         `json:"success"`
	State   SessionState `json:"state"`
	HTML    string       `json:"html,omitempty"`
}

type ExecuteRequest struct {
	Actions         []session.Action `json:"actions,omitempty"`
	ContinueOnError bool             `json:"continue_on_error,omitempty"`
	ReturnHTML      bool             `json:"return_html,omitempty"`
}

type ExecuteResponse struct {
	Success bool                   `json:"success"`
	State   SessionState           `json:"state"`
	Results []session.ActionResult `json:"results"`
	HTML    string                 `json:"html,omitempty"`
}

type HTMLResponse struct {
	Success    bool   `json:"success"`
	CurrentURL string `json:"current_url,omitempty"`
	HTML       string `json:"html,omitempty"`
}

type LiveResponse struct {
	Success       bool   `json:"success"`
	CurrentURL    string `json:"current_url,omitempty"`
	ScreenshotB64 string `json:"screenshot_b64,omitempty"`
}

type Runtime interface {
	Create(context.Context, CreateRequest) (*CreateResponse, error)
	Get(context.Context, string) (*SessionState, error)
	List(context.Context) ([]session.SessionInfo, error)
	Execute(context.Context, string, ExecuteRequest) (*ExecuteResponse, error)
	HTML(context.Context, string) (*HTMLResponse, error)
	Live(context.Context, string) (*LiveResponse, error)
	Delete(context.Context, string) error
	Close() error
}
