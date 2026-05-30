package jobs

import "errors"

var (
	// ErrJobNotFound is returned when a job cannot be found
	ErrJobNotFound = errors.New("job not found")

	// ErrJobExists is returned when trying to create a job that already exists
	ErrJobExists = errors.New("job already exists")

	// ErrInvalidID is returned when a job ID is invalid or missing
	ErrInvalidID = errors.New("invalid or missing job id")
)
