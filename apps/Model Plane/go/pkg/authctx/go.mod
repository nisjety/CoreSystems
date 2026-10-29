module github.com/triodelab/model-plane/pkg/authctx

go 1.25.1

require (
	github.com/golang-jwt/jwt/v5 v5.3.0
	google.golang.org/grpc v1.80.0
)

require (
	go.opentelemetry.io/otel v1.43.0 // indirect
	go.opentelemetry.io/otel/sdk/metric v1.43.0 // indirect
	golang.org/x/net v0.53.0 // indirect
	golang.org/x/sys v0.43.0 // indirect
	golang.org/x/text v0.36.0 // indirect
	google.golang.org/genproto/googleapis/rpc v0.0.0-20260414002931-afd174a4e478 // indirect
	google.golang.org/protobuf v1.36.11 // indirect
)

replace google.golang.org/genproto => google.golang.org/genproto v0.0.0-20260414002931-afd174a4e478
