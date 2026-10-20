//go:build e2e

// Command idp is the stub identity provider of the browser-broker e2e harness.
//
//	idp keygen <dir>     generate a key; write <dir>/private.pem (0600) and <dir>/jwks.json
//	idp serve -jwks F    serve F at /jwks.json and /.well-known/jwks.json
package main

import (
	"flag"
	"fmt"
	"log"
	"net/http"
	"os"
	"path/filepath"
	"time"

	"github.com/triodelab/model-plane/services/browser-broker/e2e/idp"
)

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintln(os.Stderr, "usage: idp keygen <dir> | idp serve -addr :3000 -jwks FILE")
		os.Exit(2)
	}
	switch os.Args[1] {
	case "keygen":
		keygen(os.Args[2:])
	case "serve":
		serve(os.Args[2:])
	default:
		fmt.Fprintln(os.Stderr, "unknown command")
		os.Exit(2)
	}
}

func keygen(args []string) {
	if len(args) != 1 {
		fmt.Fprintln(os.Stderr, "usage: idp keygen <dir>")
		os.Exit(2)
	}
	dir := args[0]
	key, err := idp.Generate(idp.DefaultKID)
	if err != nil {
		log.Fatal("generate key failed")
	}
	if err := key.WritePrivatePEM(filepath.Join(dir, "private.pem")); err != nil {
		log.Fatal("write private key failed")
	}
	if err := os.WriteFile(filepath.Join(dir, "jwks.json"), key.JWKS(), 0o644); err != nil {
		log.Fatal("write jwks failed")
	}
	fmt.Println("idp keygen: key and jwks written")
}

func serve(args []string) {
	fs := flag.NewFlagSet("serve", flag.ExitOnError)
	addr := fs.String("addr", ":3000", "listen address")
	jwksFile := fs.String("jwks", "/jwks.json", "JWKS document to serve")
	_ = fs.Parse(args)
	jwks, err := os.ReadFile(*jwksFile)
	if err != nil {
		log.Fatal("read jwks failed")
	}
	srv := &http.Server{Addr: *addr, Handler: idp.Handler(jwks), ReadHeaderTimeout: 5 * time.Second}
	fmt.Println("idp listening")
	log.Fatal(srv.ListenAndServe())
}
