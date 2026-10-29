package http

import (
	"encoding/json"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/stretchr/testify/require"
)

func TestParsePagination_Defaults(t *testing.T) {
	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Request = httptest.NewRequest("GET", "/v1/plans/thread/t1", nil)

	limit, offset := parsePagination(c)
	require.Equal(t, 50, limit)
	require.Equal(t, 0, offset)
}

func TestParsePagination_ClampsAndNormalizes(t *testing.T) {
	gin.SetMode(gin.TestMode)
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Request = httptest.NewRequest("GET", "/v1/plans/thread/t1?limit=9999&offset=-4", nil)

	limit, offset := parsePagination(c)
	require.Equal(t, 200, limit)
	require.Equal(t, 0, offset)
}

func TestDecodeSessionEvent(t *testing.T) {
	rawCreatedAt := time.Now().UTC().Truncate(time.Second)
	in := map[string]any{
		"session_id": "sess-1",
		"sequence":   int64(4),
		"event_type": "message.sent",
		"payload":    `{"role":"user","content":"hello"}`,
		"created_at": rawCreatedAt.Format(time.RFC3339Nano),
	}

	bytes, err := json.Marshal(in)
	require.NoError(t, err)

	evt, err := decodeSessionEvent(bytes)
	require.NoError(t, err)
	require.Equal(t, "sess-1", evt.SessionID)
	require.Equal(t, int64(4), evt.Sequence)
	require.Equal(t, "message.sent", evt.EventType)
	require.Equal(t, `{"role":"user","content":"hello"}`, string(evt.Payload))
	require.WithinDuration(t, rawCreatedAt, evt.CreatedAt, time.Second)
}
