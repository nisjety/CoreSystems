package spaces

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"sort"
	"strings"
)

// Direct-message identity (WS12-S1). A DM is identified by the set of people in
// it, not by a name or a creator: the same set always resolves to the one live
// DM of the organization, whoever opens it.

const (
	// MinDMParticipants and MaxDMParticipants bound the roster, creator
	// included. Two people is a `dm`, three to nine a `group_dm`. A one-person
	// set is refused: that is the personal Space.
	MinDMParticipants = 2
	MaxDMParticipants = 9

	// participantSetDomain is the hash domain string. It is its own domain
	// (never "recipient-audience/v1") so a participant hash can never equal an
	// audience hash over the same ids.
	participantSetDomain = "participant-set/v1\x00"

	// ParticipantHashPrefix prefixes the hex digest.
	ParticipantHashPrefix = "sha256:"
)

// Reasons a participant set is refused (details.reason of the typed 422).
const (
	DMReasonCount        = "count"
	DMReasonDuplicate    = "duplicate"
	DMReasonSelf         = "self"
	DMReasonNotInOrg     = "not_in_org"
	DMReasonInactive     = "inactive"
	DMReasonEmpty        = "empty"
	DMReasonOwnerMissing = "owner_missing"
)

// Sentinel errors of the DM paths. Match with errors.Is: the typed errors below
// satisfy Is against the matching sentinel.
var (
	// ErrDMDisabled: SPACE_DM_REGISTRATION is off on this Control instance.
	ErrDMDisabled = errors.New("direct messages are not enabled")
	// ErrDMNotEntitled: the organization lacks one of the three entitlements.
	ErrDMNotEntitled = errors.New("direct messages are not entitled for this organization")
	// ErrDMParticipantInvalid: the participant set is not a valid DM roster.
	ErrDMParticipantInvalid = errors.New("direct message participants are invalid")
	// ErrDMExists: a live DM with the same participant set is already registered.
	ErrDMExists = errors.New("a direct message with these participants already exists")
	// ErrFixedRoster: a DM roster is fixed at registration and never replaced.
	ErrFixedRoster = errors.New("a direct message roster is fixed at registration")
)

// DMParticipantInvalidError carries the closed reason a set was refused.
type DMParticipantInvalidError struct{ Reason string }

func (e *DMParticipantInvalidError) Error() string {
	return "direct message participants are invalid: " + e.Reason
}

func (e *DMParticipantInvalidError) Is(target error) bool { return target == ErrDMParticipantInvalid }

// DMNotEntitledError lists the entitlements that are off. Missing holds
// catalogue-style names (`dm_create_entitled`, `thread_create_entitled`,
// `thread_read_entitled`) and never a value.
type DMNotEntitledError struct{ Missing []string }

func (e *DMNotEntitledError) Error() string {
	return "direct messages are not entitled: " + strings.Join(e.Missing, ",")
}

func (e *DMNotEntitledError) Is(target error) bool { return target == ErrDMNotEntitled }

// DMKindForCount maps a total participant count (creator included) to a kind.
func DMKindForCount(count int) (Kind, error) {
	switch {
	case count == MinDMParticipants:
		return KindDM, nil
	case count > MinDMParticipants && count <= MaxDMParticipants:
		return KindGroupDM, nil
	default:
		return "", &DMParticipantInvalidError{Reason: DMReasonCount}
	}
}

// CanonicalParticipants trims each id and returns them in byte order. Ids are
// Better Auth ids compared as bytes, never with a locale collation. It refuses
// an empty id or a duplicate, and a count outside 2..9.
func CanonicalParticipants(ids []string) ([]string, error) {
	if len(ids) < MinDMParticipants || len(ids) > MaxDMParticipants {
		return nil, &DMParticipantInvalidError{Reason: DMReasonCount}
	}
	canonical := make([]string, 0, len(ids))
	seen := make(map[string]struct{}, len(ids))
	for _, id := range ids {
		id = strings.TrimSpace(id)
		if id == "" {
			return nil, &DMParticipantInvalidError{Reason: DMReasonEmpty}
		}
		if _, dup := seen[id]; dup {
			return nil, &DMParticipantInvalidError{Reason: DMReasonDuplicate}
		}
		seen[id] = struct{}{}
		canonical = append(canonical, id)
	}
	sort.Strings(canonical)
	return canonical, nil
}

// ParticipantSetHash is the content-free commitment to a participant set:
//
//	"sha256:" + hex(SHA-256("participant-set/v1\0" ||
//	    for each id in byte-sorted unique order: uint64be(len(id)) || id))
//
// It is the encoding of RecipientAudienceHash with its own domain string. The
// kind is not an input: it follows from the count. The shared vector file
// testdata/participant-set-vectors.v1.json pins the encoding for every plane.
func ParticipantSetHash(ids ...string) (string, error) {
	canonical, err := CanonicalParticipants(ids)
	if err != nil {
		return "", err
	}
	hash := sha256.New()
	hash.Write([]byte(participantSetDomain))
	for _, id := range canonical {
		var length [8]byte
		binary.BigEndian.PutUint64(length[:], uint64(len(id)))
		hash.Write(length[:])
		hash.Write([]byte(id))
	}
	return ParticipantHashPrefix + hex.EncodeToString(hash.Sum(nil)), nil
}
