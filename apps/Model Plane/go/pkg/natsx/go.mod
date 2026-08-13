module github.com/triodelab/model-plane/pkg/natsx

go 1.25.0

require github.com/triodelab/model-plane/pkg/envelope v0.0.0

require (
	github.com/klauspost/cpuid/v2 v2.2.5 // indirect
	github.com/zeebo/blake3 v0.2.4 // indirect
	golang.org/x/sys v0.43.0 // indirect
)

replace github.com/triodelab/model-plane/pkg/envelope => ../envelope
