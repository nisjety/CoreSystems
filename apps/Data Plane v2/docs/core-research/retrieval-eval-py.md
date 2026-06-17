# retrieval-eval-py Research Dive

Generated: 2026-06-07

Scope: `apps/Data Plane v2/services/retrieval-eval-py`

## Snapshot

`retrieval-eval-py` is not currently a populated runtime service. The directory exists, but the tree is effectively empty in the current workspace.

Current evidence highlights:

- current file count is `0`
- multiple docs still mention it as a scaffold or benchmark target
- the live quality surface in this plane is `data-quality-go`

## Runtime Shape

No runtime entrypoint was found in this pass:

- no `main.py`
- no package manifest
- no populated lab or service code under the directory

## Relationship Read

Current relationship state:

- docs -> `retrieval-eval-py`
  - historical or planned scaffold references
- runtime -> `data-quality-go`
  - actual active eval surface

## Cleanup Read

Do not delete the directory blindly unless upstream tooling confirms it is unused.

Update or archive, not delete:

- docs that describe `retrieval-eval-py` as if it were an active service

## Bottom Line

This is a documentation and planning residue point, not a live core. The right next decision is explicit:

- either create the Python harness for real
- or stop describing it as present runtime state
