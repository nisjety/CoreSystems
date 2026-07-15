package clients

import "net/http"

func setSessionServicePrincipal(request *http.Request, token string) {
	if token == "" {
		return
	}
	request.Header.Set("X-Service-Id", "session-core")
	request.Header.Set("X-Service-Token", token)
}

func rejectScopedServiceRedirect(_ *http.Request, _ []*http.Request) error {
	return http.ErrUseLastResponse
}
