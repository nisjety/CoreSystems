package browser

import (
	"strings"

	"github.com/gofiber/fiber/v2"
)

type Server struct {
	runtime     Runtime
	internalKey string
}

func NewServer(runtime Runtime, internalKey string) *Server {
	return &Server{
		runtime:     runtime,
		internalKey: strings.TrimSpace(internalKey),
	}
}

func (s *Server) Register(app *fiber.App) {
	app.Get("/health", func(c *fiber.Ctx) error {
		return c.JSON(fiber.Map{"success": true, "service": "quarry-browser", "status": "ok"})
	})

	internal := app.Group("/internal/browser", s.authorize)
	internal.Post("/sessions", s.createSession)
	internal.Get("/sessions", s.listSessions)
	internal.Get("/sessions/:id", s.getSession)
	internal.Get("/sessions/:id/html", s.getHTML)
	internal.Get("/sessions/:id/live", s.getLive)
	internal.Post("/sessions/:id/execute", s.execute)
	internal.Delete("/sessions/:id", s.deleteSession)
}

func (s *Server) authorize(c *fiber.Ctx) error {
	if s == nil || s.internalKey == "" {
		return c.Next()
	}
	if strings.TrimSpace(c.Get("X-Internal-API-Key")) != s.internalKey {
		return c.Status(fiber.StatusUnauthorized).JSON(fiber.Map{
			"success": false,
			"error":   "invalid internal api key",
		})
	}
	return c.Next()
}

func (s *Server) createSession(c *fiber.Ctx) error {
	var req CreateRequest
	if err := c.BodyParser(&req); err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{"success": false, "error": "invalid request body"})
	}
	resp, err := s.runtime.Create(c.UserContext(), req)
	if err != nil {
		return c.Status(fiber.StatusBadGateway).JSON(fiber.Map{"success": false, "error": err.Error()})
	}
	return c.Status(fiber.StatusCreated).JSON(resp)
}

func (s *Server) listSessions(c *fiber.Ctx) error {
	data, err := s.runtime.List(c.UserContext())
	if err != nil {
		return c.Status(fiber.StatusServiceUnavailable).JSON(fiber.Map{"success": false, "error": err.Error()})
	}
	return c.JSON(fiber.Map{"success": true, "data": data, "count": len(data)})
}

func (s *Server) getSession(c *fiber.Ctx) error {
	state, err := s.runtime.Get(c.UserContext(), strings.TrimSpace(c.Params("id")))
	if err != nil {
		return c.Status(fiber.StatusNotFound).JSON(fiber.Map{"success": false, "error": err.Error()})
	}
	return c.JSON(fiber.Map{"success": true, "state": state})
}

func (s *Server) getHTML(c *fiber.Ctx) error {
	response, err := s.runtime.HTML(c.UserContext(), strings.TrimSpace(c.Params("id")))
	if err != nil {
		return c.Status(fiber.StatusNotFound).JSON(fiber.Map{"success": false, "error": err.Error()})
	}
	return c.JSON(response)
}

func (s *Server) getLive(c *fiber.Ctx) error {
	response, err := s.runtime.Live(c.UserContext(), strings.TrimSpace(c.Params("id")))
	if err != nil {
		return c.Status(fiber.StatusNotFound).JSON(fiber.Map{"success": false, "error": err.Error()})
	}
	return c.JSON(response)
}

func (s *Server) execute(c *fiber.Ctx) error {
	var req ExecuteRequest
	if err := c.BodyParser(&req); err != nil {
		return c.Status(fiber.StatusBadRequest).JSON(fiber.Map{"success": false, "error": "invalid request body"})
	}
	response, err := s.runtime.Execute(c.UserContext(), strings.TrimSpace(c.Params("id")), req)
	if err != nil {
		return c.Status(fiber.StatusNotFound).JSON(fiber.Map{"success": false, "error": err.Error()})
	}
	return c.JSON(response)
}

func (s *Server) deleteSession(c *fiber.Ctx) error {
	if err := s.runtime.Delete(c.UserContext(), strings.TrimSpace(c.Params("id"))); err != nil {
		return c.Status(fiber.StatusNotFound).JSON(fiber.Map{"success": false, "error": err.Error()})
	}
	return c.JSON(fiber.Map{"success": true})
}
