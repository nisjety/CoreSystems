package actions

import (
	"context"

	"github.com/triodelab/quarry/internal/driver"
)

func Execute(ctx context.Context, drv driver.PageDriver, steps []ActionStep) ([]ActionResult, error) {
	engine := NewEngine()
	return engine.Execute(ctx, drv, steps)
}
