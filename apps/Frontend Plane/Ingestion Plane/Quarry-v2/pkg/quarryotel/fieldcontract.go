package quarryotel

// The span field vocabulary (WS10.S8). These constants must equal the golden
// crates/quarry-runtime/contracts/telemetry-field-contract.json; testdata/
// holds a byte-identical copy and TestFieldConstantsEqualGolden compares them.
// The golden is the single source: change it there first, then mirror here.

// ExportEnv turns span export on (default) or off (`off`, `0`, `false`).
const ExportEnv = "QUARRY_TELEMETRY_EXPORT"

// RegistryCapEnv sizes the ZDR trace registry (a positive integer).
const RegistryCapEnv = "QUARRY_TELEMETRY_ZDR_REGISTRY_CAP"

// DefaultRegistryCap is the registry bound when RegistryCapEnv is unset or invalid.
const DefaultRegistryCap = 100000

// ZdrAttribute is the span attribute that marks a ZDR span.
const ZdrAttribute = "zdr"

// SafeFields may stay on any span.
var SafeFields = []string{
	"request_id",
	"run_id",
	"step",
	"zdr",
	"outcome",
	"error_class",
	"status",
	"status_code",
	"driver",
	"driver_name",
	"attempt",
	"count",
	"bytes",
	"byte_size",
	"duration_ms",
	"elapsed_ms",
	"kind",
	"method",
	"route",
}

// ZdrSensitiveFields are removed from every span that is kept (WS4 SENSITIVE_FIELDS).
var ZdrSensitiveFields = []string{
	"url",
	"uri",
	"source_url",
	"target_url",
	"target",
	"query",
	"q",
	"selector",
	"title",
	"host",
	"domain",
	"origin",
	"snippet",
	"text",
	"markdown",
	"excerpt",
	"error",
	"err",
	"reason_text",
}

// NeverFields never leave the process, ZDR or not.
var NeverFields = []string{
	"authorization",
	"cookie",
	"set-cookie",
	"token",
	"password",
	"otp",
	"secret",
	"api_key",
	"typed_text",
}

var strippedKeys = func() map[string]struct{} {
	keys := make(map[string]struct{}, len(ZdrSensitiveFields)+len(NeverFields))
	for _, k := range ZdrSensitiveFields {
		keys[k] = struct{}{}
	}
	for _, k := range NeverFields {
		keys[k] = struct{}{}
	}
	return keys
}()

// IsStrippedKey reports whether an attribute key is removed from a kept span.
func IsStrippedKey(key string) bool {
	_, ok := strippedKeys[key]
	return ok
}
