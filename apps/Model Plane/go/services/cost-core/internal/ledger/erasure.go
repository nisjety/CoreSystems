package ledger

import (
	"crypto/sha256"
	"encoding/hex"
	"errors"
)

var ErrOrgErased = errors.New("organization ledger has been erased")

func ErasedOrgHash(orgID string) string {
	h := sha256.Sum256([]byte("cost-core-erased-org:" + orgID))
	return hex.EncodeToString(h[:])
}
