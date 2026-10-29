module github.com/triodelab/anbud

go 1.25.0

require (
	github.com/triodelab/studio-plane/planeclient v0.0.0
	github.com/gofiber/fiber/v2 v2.52.13
	github.com/jackc/pgx/v5 v5.9.2
	github.com/ledongthuc/pdf v0.0.0-20260907135840-6c8c28e0e8a0
	github.com/rs/zerolog v1.34.0
	github.com/valyala/fasthttp v1.70.0
	golang.org/x/net v0.58.0
	golang.org/x/text v0.41.0
	golang.org/x/time v0.15.0
)

replace github.com/triodelab/studio-plane/planeclient => ../pkg/planeclient

require (
	github.com/andybalholm/brotli v1.2.1 // indirect
	github.com/google/uuid v1.6.0 // indirect
	github.com/jackc/pgpassfile v1.0.0 // indirect
	github.com/jackc/pgservicefile v0.0.0-20240606120523-5a60cdf6a761 // indirect
	github.com/jackc/puddle/v2 v2.2.2 // indirect
	github.com/klauspost/compress v1.18.7 // indirect
	github.com/mattn/go-colorable v0.1.13 // indirect
	github.com/mattn/go-isatty v0.0.20 // indirect
	github.com/mattn/go-runewidth v0.0.16 // indirect
	github.com/rivo/uniseg v0.2.0 // indirect
	github.com/valyala/bytebufferpool v1.0.0 // indirect
	golang.org/x/sync v0.22.0 // indirect
	golang.org/x/sys v0.47.0 // indirect
)
