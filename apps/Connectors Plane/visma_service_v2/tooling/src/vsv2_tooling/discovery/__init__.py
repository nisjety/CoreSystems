"""Phase 0 — discovery & gating.

Commands, all callable through the `vsv2-discover` Typer app:

    vsv2-discover introspect   # GraphQL introspection → schema/datamart.sdl
    vsv2-discover companies    # list companies → fill tenant_company_map
    vsv2-discover coverage     # build bronze ↔ DM coverage matrix
    vsv2-discover probe        # rate-limit + pagination probe
    vsv2-discover generate-queries
                               # materialize live DM query catalog + mapping report
    vsv2-discover validate-queries
                               # validate generated DM queries against live schema
    vsv2-discover review-candidates
                               # review synchronized DM mappings against bronze
    vsv2-discover shadow-compare
                               # live ERP vs DataMart sample comparison
"""
