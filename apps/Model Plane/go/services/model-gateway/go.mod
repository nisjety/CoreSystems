module github.com/triodelab/model-plane/services/model-gateway

go 1.25.1

require (
	github.com/triodelab/model-plane/gen/go v0.0.0
	github.com/triodelab/model-plane/pkg/quarry v0.0.0
	google.golang.org/grpc v1.80.0
	google.golang.org/protobuf v1.36.11
)

require (
	go.opentelemetry.io/otel/metric v1.43.0 // indirect
	go.opentelemetry.io/otel/sdk v1.43.0 // indirect
	go.opentelemetry.io/otel/trace v1.43.0 // indirect
	golang.org/x/net v0.52.0 // indirect
	golang.org/x/sys v0.42.0 // indirect
	golang.org/x/text v0.35.0 // indirect
	google.golang.org/genproto/googleapis/rpc v0.0.0-20260414002931-afd174a4e478 // indirect
)

replace github.com/triodelab/model-plane/gen/go => ../../gen

replace github.com/triodelab/model-plane/pkg/quarry => ../../pkg/quarry

replace google.golang.org/genproto => google.golang.org/genproto v0.0.0-20260414002931-afd174a4e478
