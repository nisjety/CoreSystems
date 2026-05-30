module github.com/triodelab/model-plane/pkg/natsx

go 1.22.0

require (
	github.com/nats-io/nats.go v1.37.0
	github.com/triodelab/model-plane/pkg/envelope v0.0.0
)

replace github.com/triodelab/model-plane/pkg/envelope => ../envelope
