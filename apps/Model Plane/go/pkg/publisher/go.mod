module github.com/triodelab/model-plane/pkg/publisher

go 1.22.0

require github.com/triodelab/model-plane/pkg/envelope v0.0.0

require (
	github.com/klauspost/cpuid/v2 v2.0.12 // indirect
	github.com/zeebo/blake3 v0.2.4 // indirect
)

replace github.com/triodelab/model-plane/pkg/envelope => ../envelope
