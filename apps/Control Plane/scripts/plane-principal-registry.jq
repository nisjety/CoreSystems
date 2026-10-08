# plane-principal-registry.jq -- the ONE definition of how the committed principal
# policy (config/plane-service-principals.json) becomes the registry Auth Core
# loads (PLANE_SERVICE_PRINCIPALS_JSON).
#
# Two writers used to carry their own copy of this: the launcher
# (scripts/run-control-plane.sh) and the central bootstrap
# (scripts/bootstrap_runtime_environment.sh, which kept a third, inline registry
# that had drifted: 14 principals, older scopes, no tokenExchange). A re-bootstrap
# silently reverted policy, and the launcher then adopted the stale copy's
# credentials. Both now call this filter, and scripts/tests/
# principal-registry-parity.test.sh fails when the committed policy and what
# either writer emits disagree.
#
# Run with -n and:
#   --slurpfile pol   config/plane-service-principals.json     (one document)
#   --argjson   creds {"<principal>": "<credential>", ...}     (one per principal)
#   --arg       profile development | production
#
# Profiles. The policy states each principal's organizations the production way:
# `tenancy: "directory"` (any organization org-core's directory holds, verified
# by Auth Core at every mint) and/or a fixed `orgIds` list.
#
#   production    emitted as the policy states it. A row carrying
#                 `allowAnyOrg: true` is refused here, because Auth Core's
#                 production loader would refuse the whole registry.
#   development   a `tenancy: "directory"` row is emitted as the legacy
#                 `allowAnyOrg: true`, which is what the development stack has
#                 always run on (no org-core lookup on the mint path, pseudo- and
#                 fixture organizations unchecked). Nothing about development
#                 changes; set PLANE_SERVICE_PRINCIPALS_PROFILE=production in the
#                 launcher to run the production binding locally.
#
# Keys the policy does not carry are OMITTED, never emitted as null: Auth Core
# reads an absent key as "use the default" but rejects an explicit null, and it
# parses the whole registry on every request, so one null 503s every mint fleet
# wide while /health stays green.
#
# The same goes for every other per-entry rule Auth Core's loader enforces
# (plane-service-principal.ts: nonEmptyStrings, validAudienceScopeMap,
# validAudienceRetentionMap, validTokenExchangeMap). A row that breaks one is refused
# HERE, by name, rather than emitted: the registry would be refused whole at the other
# end. The assembly contract spec
# (auth-core/src/auth/plane-principal-registry-assembly.contract.spec.ts) pins this.

def fail($name; $why):
  error("plane-service-principals.json: principal " + $name + ": " + $why);

def has_org_source($row):
  (($row.tenancy // null) == "directory")
  or (($row.allowAnyOrg // false) == true)
  or (($row.orgIds // []) | length > 0);

def org_form($name; $row; $profile):
  ($row.tenancy // null) as $tenancy
  | ($row | has("allowAnyOrg")) as $declaresAny
  | if $tenancy != null and $tenancy != "directory"
    then fail($name; "tenancy must be \"directory\" when present")
    elif (has_org_source($row) | not)
    then fail($name; "names no organization source (tenancy \"directory\", a non-empty orgIds, or allowAnyOrg)")
    elif $profile == "development"
    then (if $tenancy == "directory" then {allowAnyOrg: true}
          elif $declaresAny then {allowAnyOrg: $row.allowAnyOrg}
          else {} end)
    elif $profile == "production"
    then (if ($row.allowAnyOrg // false) == true
          then fail($name; "allowAnyOrg is refused in the production profile; use tenancy \"directory\" or a fixed orgIds list")
          else (if $tenancy == "directory" then {tenancy: "directory"} else {} end)
               + (if $declaresAny then {allowAnyOrg: $row.allowAnyOrg} else {} end)
          end)
    else error("unknown principal registry profile: " + ($profile | tostring))
    end;

def optional($row; $key):
  if $row | has($key) then {($key): $row[$key]} else {} end;

# What Auth Core's own loader means by a list of names: a non-empty array whose every
# element is a string with something in it (nonEmptyStrings in plane-service-principal.ts).
def has_text: type == "string" and test("\\S");
def non_empty_strings: type == "array" and length > 0 and all(.[]; has_text);

def required_array($name; $row; $key):
  ($row[$key] // null) as $value
  | if ($value | non_empty_strings) then $value
    else fail($name; "\"" + $key + "\" must be a non-empty array of non-empty strings") end;

# `orgIds` is always emitted (Auth Core requires an array, and an absent or null
# one invalidates the whole registry), but a directory row need not list any: an
# absent key is the empty list, a value that is not a list is an error here
# instead of a fleet-wide 503 there.
def org_ids($name; $row):
  ($row.orgIds // []) as $value
  | if ($value | type) == "array" and all($value[]; has_text) then $value
    else fail($name; "\"orgIds\" must be an array of non-empty strings") end;

# The per-entry invariants Auth Core checks on EVERY request and answers, for one
# violation anywhere, by refusing the whole registry (503 on every mint on every plane
# while /health stays green). The commonest hand edit that breaks them is adding an
# audience to a row and forgetting its entry in scopesByAudience or retentionByAudience.
# Checked here so the launcher and the bootstrap stop instead (the same rules as
# validAudienceScopeMap, validAudienceRetentionMap and validTokenExchangeMap).
def check_maps($name; $row; $audiences; $scopes):
  (if ($row | has("scopesByAudience")) then
       $row.scopesByAudience as $m
       | if ($m | type) != "object" then fail($name; "\"scopesByAudience\" must be an object")
         elif ($m | keys | sort) != $audiences then fail($name; "\"scopesByAudience\" must have exactly one entry for each audience")
         elif ([$m[] | non_empty_strings] | all | not) then fail($name; "every \"scopesByAudience\" entry must be a non-empty array of non-empty strings")
         elif ([$m[][]] | unique) != $scopes then fail($name; "the union of \"scopesByAudience\" must equal \"scopes\"")
         else . end
     else . end)
  | (if ($row | has("retentionByAudience")) then
       $row.retentionByAudience as $m
       | if ($m | type) != "object" then fail($name; "\"retentionByAudience\" must be an object")
         elif ($m | keys | sort) != $audiences then fail($name; "\"retentionByAudience\" must have exactly one entry for each audience")
         elif ([$m[] | IN("zdr", "persistent", "tenant")] | all | not) then fail($name; "\"retentionByAudience\" values must be zdr, persistent or tenant")
         else . end
     else . end)
  | (if ($row | has("tokenExchange")) then
       $row.tokenExchange as $m
       | if ($m | type) != "object" then fail($name; "\"tokenExchange\" must be an object")
         elif ([$m | keys[] | IN($audiences[])] | all | not) then fail($name; "every \"tokenExchange\" target must be one of the row's audiences")
         elif ([$m[] | non_empty_strings] | all | not) then fail($name; "every \"tokenExchange\" entry must be a non-empty array of non-empty strings")
         else . end
     else . end);

# The only keys a policy row may carry. Anything else is refused rather than dropped: a
# row whose per-audience map is misspelled (`scopesByAudiance`) would otherwise be
# emitted without one, and Auth Core then grants EVERY scope the row lists on EVERY
# audience it lists, silently widening the principal to the union of its scopes.
def known_row_keys:
  ["allowAnyOrg", "audiences", "consumerEnv", "orgIds", "retentionByAudience",
   "scopes", "scopesByAudience", "tenancy", "tokenExchange"];

def check_keys($name; $row):
  ([$row | keys[] | select(IN(known_row_keys[]) | not)]) as $unknown
  | if ($unknown | length) > 0
    then fail($name; "unknown key(s) " + ($unknown | join(", ")) + "; a misspelled key is refused, never dropped")
    # `allowAnyOrg` is copied through when the row declares it; Auth Core refuses the
    # whole registry for one that is not a boolean (an explicit null included).
    elif ($row | has("allowAnyOrg")) and (($row.allowAnyOrg | type) != "boolean")
    then fail($name; "\"allowAnyOrg\" must be true or false when present")
    else . end;

def credential($name; $creds):
  ($creds[$name] // null) as $value
  | if ($value | type) == "string" and ($value | length) >= 16 then $value
    else fail($name; "no credential of at least 16 characters was supplied") end;

reduce ($pol[0] | keys[]) as $name ({};
  ($pol[0][$name]) as $row
  | (if ($row | type) == "object" then . else fail($name; "the policy row must be an object") end)
  | ($row | check_keys($name; $row)) as $_
  | required_array($name; $row; "audiences") as $audiences
  | required_array($name; $row; "scopes") as $scopes
  | ($row | check_maps($name; $row; $audiences | unique; $scopes | unique)) as $_
  | .[$name] = (
      {credential: credential($name; $creds),
       audiences: required_array($name; $row; "audiences"),
       orgIds: org_ids($name; $row)}
      + org_form($name; $row; $profile)
      + {scopes: required_array($name; $row; "scopes")}
      + optional($row; "scopesByAudience")
      + optional($row; "retentionByAudience")
      + optional($row; "tokenExchange")
    )
)
