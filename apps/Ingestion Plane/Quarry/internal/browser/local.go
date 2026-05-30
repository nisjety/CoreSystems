package browser

import (
	"context"
	"fmt"

	"github.com/triodelab/quarry/internal/session"
)

type LocalRuntime struct {
	manager *session.Manager
}

func NewLocalRuntime(manager *session.Manager) *LocalRuntime {
	return &LocalRuntime{manager: manager}
}

func (r *LocalRuntime) Create(ctx context.Context, req CreateRequest) (*CreateResponse, error) {
	if r == nil || r.manager == nil {
		return nil, fmt.Errorf("session manager not initialized")
	}
	sess, err := r.manager.Create(ctx, req.URL, &session.CreateOptions{
		Viewport: req.Viewport,
		Mobile:   req.Mobile,
		Profile:  req.Profile,
		OrgID:    req.OrgID,
		UserID:   req.UserID,
	})
	if err != nil {
		return nil, err
	}
	html, _ := sess.HTML(ctx)
	return &CreateResponse{
		Success: true,
		State:   sessionState(sess),
		HTML:    html,
	}, nil
}

func (r *LocalRuntime) Get(_ context.Context, id string) (*SessionState, error) {
	if r == nil || r.manager == nil {
		return nil, fmt.Errorf("session manager not initialized")
	}
	sess, err := r.manager.Get(id)
	if err != nil {
		return nil, err
	}
	state := sessionState(sess)
	return &state, nil
}

func (r *LocalRuntime) List(_ context.Context) ([]session.SessionInfo, error) {
	if r == nil || r.manager == nil {
		return nil, fmt.Errorf("session manager not initialized")
	}
	return r.manager.List(), nil
}

func (r *LocalRuntime) Execute(ctx context.Context, id string, req ExecuteRequest) (*ExecuteResponse, error) {
	if r == nil || r.manager == nil {
		return nil, fmt.Errorf("session manager not initialized")
	}
	sess, err := r.manager.Get(id)
	if err != nil {
		return nil, err
	}
	results := sess.ExecuteBatch(ctx, req.Actions, req.ContinueOnError)
	html := ""
	if req.ReturnHTML {
		html, _ = sess.HTML(ctx)
	}
	return &ExecuteResponse{
		Success: true,
		State:   sessionState(sess),
		Results: results,
		HTML:    html,
	}, nil
}

func (r *LocalRuntime) HTML(ctx context.Context, id string) (*HTMLResponse, error) {
	if r == nil || r.manager == nil {
		return nil, fmt.Errorf("session manager not initialized")
	}
	sess, err := r.manager.Get(id)
	if err != nil {
		return nil, err
	}
	html, err := sess.HTML(ctx)
	if err != nil {
		return nil, err
	}
	return &HTMLResponse{
		Success:    true,
		CurrentURL: sess.CurrentURL(),
		HTML:       html,
	}, nil
}

func (r *LocalRuntime) Live(ctx context.Context, id string) (*LiveResponse, error) {
	if r == nil || r.manager == nil {
		return nil, fmt.Errorf("session manager not initialized")
	}
	sess, err := r.manager.Get(id)
	if err != nil {
		return nil, err
	}
	screenshotB64, err := sess.Screenshot(ctx, true)
	if err != nil {
		return nil, err
	}
	return &LiveResponse{
		Success:       true,
		CurrentURL:    sess.CurrentURL(),
		ScreenshotB64: screenshotB64,
	}, nil
}

func (r *LocalRuntime) Delete(_ context.Context, id string) error {
	if r == nil || r.manager == nil {
		return fmt.Errorf("session manager not initialized")
	}
	r.manager.Destroy(id)
	return nil
}

func (r *LocalRuntime) Close() error {
	return nil
}

func sessionState(sess *session.BrowserSession) SessionState {
	if sess == nil {
		return SessionState{}
	}
	return SessionState{
		Session: session.SessionInfo{
			ID:        sess.ID,
			URL:       sess.URL,
			Profile:   sess.Profile,
			CreatedAt: sess.CreatedAt,
			ExpiresAt: sess.ExpiresAt,
			LastUsed:  sess.LastUsed,
			StepCount: sess.StepCount,
		},
		CurrentURL: sess.CurrentURL(),
	}
}
