// Package zdrrefusal is the Go side of the Model Plane zero-data-retention
// refusal catalogue (apps/Model Plane/api/contracts/zdr-refusals.json, written
// by mp-contracts' zdr_refusal tests).
//
// A Go service that refuses a ZDR caller answers FailedPrecondition with the
// catalogue's kind trailer (and, for a feature code, the feature trailer), the
// same wire shape mp-contracts' Refusal::grpc_status writes, so execution-core
// and model-gateway turn it into the typed JSON body without a mapping of their
// own. The catalogue is a vendored, embedded copy: scripts/tests/
// zdr-contract-sync.test.sh keeps it byte-identical to the owner's file, and
// this package never invents a code or a sentence of its own.
package zdrrefusal

import (
	"context"
	_ "embed"
	"encoding/json"
	"strings"
	"sync"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

// Catalogue codes this package can raise. Values are the catalogue's, verbatim.
const (
	DurableMutationForbidden = "zdr_durable_mutation_forbidden"
	FeatureNotSupported      = "zdr_feature_not_supported"
	PostureMismatch          = "zdr_posture_mismatch"
	DecisionUnavailable      = "zdr_decision_unavailable"
)

const (
	// KindTrailer names the catalogue code on a gRPC status.
	KindTrailer = "x-mp-provider-error"
	// FeatureTrailer names the feature, sent only for a code that takes one.
	FeatureTrailer = "x-mp-zdr-feature"
)

//go:embed zdr-refusals.json
var catalogueJSON []byte

type entry struct {
	Code         string `json:"code"`
	Message      string `json:"message_en"`
	TakesFeature bool   `json:"takes_feature"`
}

type featureEntry struct {
	Key   string `json:"key"`
	Label string `json:"label_en"`
}

type catalogue struct {
	Codes    []entry        `json:"codes"`
	Features []featureEntry `json:"features"`
}

var (
	loadOnce sync.Once
	loaded   catalogue
	loadErr  error
)

func load() (catalogue, error) {
	loadOnce.Do(func() { loadErr = json.Unmarshal(catalogueJSON, &loaded) })
	return loaded, loadErr
}

// Known reports whether code is a catalogue code.
func Known(code string) bool {
	_, ok := lookup(code)
	return ok
}

func lookup(code string) (entry, bool) {
	cat, err := load()
	if err != nil {
		return entry{}, false
	}
	for _, candidate := range cat.Codes {
		if candidate.Code == code {
			return candidate, true
		}
	}
	return entry{}, false
}

// Message is the catalogue sentence for code, with {feature} replaced by the
// feature's label when the code takes one. An unknown code yields "".
func Message(code, feature string) string {
	found, ok := lookup(code)
	if !ok {
		return ""
	}
	if !found.TakesFeature {
		return found.Message
	}
	label := feature
	if cat, err := load(); err == nil {
		for _, candidate := range cat.Features {
			if candidate.Key == feature {
				label = candidate.Label
				break
			}
		}
	}
	return strings.ReplaceAll(found.Message, "{feature}", label)
}

// Status is the FailedPrecondition status a refusal carries: the catalogue
// sentence as the message and the kind (and feature) trailers as metadata. A
// code outside the catalogue, or a feature code without a catalogue feature,
// degrades to a plain FailedPrecondition with no trailer, which no client can
// mistake for a typed refusal; callers use the Code constants above.
func Status(code, feature string) error {
	found, ok := lookup(code)
	if !ok {
		return status.Error(codes.FailedPrecondition, "zero data retention refusal")
	}
	if found.TakesFeature && !knownFeature(feature) {
		return status.Error(codes.FailedPrecondition, "zero data retention refusal")
	}
	return status.Error(codes.FailedPrecondition, Message(code, feature))
}

// Refuse returns Status(code, feature) after setting the trailers on the call,
// which is where mp-contracts' reader looks for them. A context with no server
// stream (a direct call in a test) just gets the status.
func Refuse(ctx context.Context, code, feature string) error {
	err := Status(code, feature)
	if !Known(code) {
		return err
	}
	pairs := []string{KindTrailer, code}
	if found, _ := lookup(code); found.TakesFeature {
		pairs = append(pairs, FeatureTrailer, feature)
	}
	_ = grpc.SetTrailer(ctx, metadata.Pairs(pairs...))
	return err
}

func knownFeature(key string) bool {
	cat, err := load()
	if err != nil {
		return false
	}
	for _, candidate := range cat.Features {
		if candidate.Key == key {
			return true
		}
	}
	return false
}
