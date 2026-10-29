// Package domain contains shared error sentinels for capability-core.
package domain

import "errors"

// ErrCapabilityNotFound is returned when a capability ID is unknown.
var ErrCapabilityNotFound = errors.New("capability not found")

// ErrVersionMismatch is returned when the requested version constraint does
// not match the stored capability version.
var ErrVersionMismatch = errors.New("capability version mismatch")

// ErrPolicyDenied is returned when the policy engine rejects an evaluation.
var ErrPolicyDenied = errors.New("policy denied")

// ErrInvalidArgument is returned for empty or malformed inputs.
var ErrInvalidArgument = errors.New("invalid argument")

// ErrPermissionDenied is returned when a subject lacks RBAC permission for a capability.
var ErrPermissionDenied = errors.New("permission denied")
