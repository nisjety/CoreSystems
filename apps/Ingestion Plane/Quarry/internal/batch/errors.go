package batch

import "errors"

var (
	ErrJobNotFound       = errors.New("job not found")
	ErrJobAlreadyStarted = errors.New("job already started")
	ErrJobTimeout        = errors.New("job timeout")
	ErrJobCancelled      = errors.New("job cancelled")
)
