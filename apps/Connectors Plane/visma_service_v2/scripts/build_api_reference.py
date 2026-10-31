#!/usr/bin/env python3
"""Derive the compact Visma API references that vsv2-mcp and vsv2-interactive embed.

usage:
  python3 scripts/build_api_reference.py --erp SPEC --erp-interactive SPEC \
      --salesorder SPEC --journaltx SPEC --retrieved YYYY-MM-DD

The four specs are Visma's public API documents:
  --erp, --erp-interactive  https://api.finance.visma.net/API-index/api/vismanet.erp.service.api-apim
                            https://api.finance.visma.net/API-index/api/vismanet.erp.interactive.api-apim
  --salesorder              https://salesorder.visma.net/swagger/v3/swagger.json   (Swagger 2.0)
  --journaltx               https://journaltx.visma.net/swagger/v1/swagger.json    (OpenAPI 3)

The script reads only the files given (no network) and writes two deterministic JSON files:

  core/crates/vsv2-mcp/reference/api_reference.json
      every operation of the three APIs: method, path, summary, tags and, for GET, the query parameter
      names. It also records whether the interactive ERP API has exactly the operations of the service one.
  core/crates/vsv2-interactive/schemas/salesorder_v3_create.json
      the request body schema of POST /api/v3/SalesOrders, references resolved to a bounded depth.

Regenerate both whenever a spec changes; the tests fail if the embedded data cannot be parsed.
"""
import argparse
import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
REFERENCE_OUT = ROOT / "core/crates/vsv2-mcp/reference/api_reference.json"
SALESORDER_CREATE_OUT = ROOT / "core/crates/vsv2-interactive/schemas/salesorder_v3_create.json"
METHODS = ("get", "post", "put", "patch", "delete")
# Keys that only describe a schema to humans; the validator and the search do not need them.
DROP_KEYS = {
    "description", "title", "example", "examples", "deprecated", "externalDocs", "xml",
    "readOnly", "writeOnly", "default", "format", "nullable", "x-nullable",
}
MAX_REF_DEPTH = 4
# Not part of the OpenAPI document: from Visma's authentication documentation, where it is called the "API base
# URL" and is the `aud` of an interactive token. It is NOT a route: the gateway answers 404 for
# https://api.finance.visma.net/erp/interactive/v1/... (checked 2026-10-14), so a signed-in person calls the same
# host and paths as the service.
ERP_INTERACTIVE_AUDIENCE = "https://api.finance.visma.net/erp/interactive"

# `arg` is the command-line option that supplies the spec; `hosts` is used when the spec has no servers.
APIS = (
    {
        "id": "erp",
        "arg": "erp",
        "title": "Visma.net ERP API (Finance)",
        "source": "https://api.finance.visma.net/API-index/api/vismanet.erp.service.api-apim",
        "hosts": ["https://api.finance.visma.net"],
    },
    {
        "id": "salesorder_v3",
        "arg": "salesorder",
        "title": "Visma.net ERP SalesOrders API v3",
        "source": "https://salesorder.visma.net/swagger/v3/swagger.json",
        "hosts": ["https://salesorder.visma.net"],
    },
    {
        "id": "journaltx_v1",
        "arg": "journaltx",
        "title": "Visma.net ERP JournalTx API v1",
        "source": "https://journaltx.visma.net/swagger/v1/swagger.json",
        "hosts": ["https://journaltx.visma.net"],
    },
)


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--erp", required=True)
    parser.add_argument("--erp-interactive", required=True)
    parser.add_argument("--salesorder", required=True)
    parser.add_argument("--journaltx", required=True)
    parser.add_argument("--retrieved", default="unknown", help="date the specs were downloaded")
    return parser.parse_args()


class Spec:
    def __init__(self, document):
        self.document = document

    @classmethod
    def load(cls, path):
        return cls(json.loads(pathlib.Path(path).read_text(encoding="utf-8")))

    def lookup(self, ref):
        if not ref.startswith("#/"):
            raise ValueError(f"external reference not supported: {ref}")
        node = self.document
        for part in ref[2:].split("/"):
            node = node[part]
        return node

    def dereference(self, node):
        while isinstance(node, dict) and "$ref" in node:
            node = self.lookup(node["$ref"])
        return node

    def schema(self, node, depth=0):
        """Resolve `$ref`s up to MAX_REF_DEPTH; deeper or composed schemas become lenient objects."""
        if not isinstance(node, dict):
            return {}
        if "$ref" in node:
            if depth >= MAX_REF_DEPTH:
                return {"type": "object"}
            return self.schema(self.lookup(node["$ref"]), depth + 1)
        if any(key in node for key in ("allOf", "oneOf", "anyOf")):
            return {"type": "object"}
        out = {}
        for key, value in node.items():
            if key in DROP_KEYS:
                continue
            if key == "properties":
                out[key] = {name: self.schema(child, depth) for name, child in value.items()}
            elif key == "items":
                out[key] = self.schema(value, depth)
            else:
                out[key] = value
        return out

    def operations(self):
        for path in sorted(self.document["paths"]):
            item = self.document["paths"][path]
            for method in METHODS:
                if method in item:
                    yield path, method, item[method]

    def query_names(self, path, op):
        item = self.document["paths"][path]
        names, required = [], []
        for raw in list(item.get("parameters", [])) + list(op.get("parameters", [])):
            param = self.dereference(raw)
            if param.get("in") != "query":
                continue
            names.append(param["name"])
            if param.get("required"):
                required.append(param["name"])
        return sorted(set(names)), sorted(set(required))

    def body_schema(self, op):
        """The JSON request body schema of an operation (OpenAPI 3 requestBody or Swagger 2 body parameter)."""
        request_body = op.get("requestBody")
        if request_body:
            content = self.dereference(request_body).get("content", {})
            media = content.get("application/json") or next(iter(content.values()), None)
            return self.schema(media["schema"]) if media and "schema" in media else None
        for raw in op.get("parameters", []):
            param = self.dereference(raw)
            if param.get("in") == "body" and "schema" in param:
                return self.schema(param["schema"])
        return None

    def hosts(self, fallback):
        servers = [s.get("url", "").rstrip("/") for s in self.document.get("servers", [])]
        return [s for s in servers if s] or list(fallback)


def signature(spec):
    """Every operation with its query parameter names: what two specs must share to be the same API."""
    return {
        (method.upper(), path): spec.query_names(path, op)[0]
        for path, method, op in spec.operations()
    }


def interactive_info(service, interactive):
    scheme = next(iter(interactive.document["components"]["securitySchemes"].values()))
    flow = scheme["flows"]["authorizationCode"]
    return {
        "same_operations": signature(service) == signature(interactive),
        "audience": ERP_INTERACTIVE_AUDIENCE,
        "authorization_url": flow["authorizationUrl"],
        "token_url": flow["tokenUrl"],
        "scopes": sorted(flow["scopes"]),
    }


def build_reference(specs, interactive, retrieved):
    apis, operations = {}, []
    for api in APIS:
        spec = specs[api["arg"]]
        entry = {
            "title": api["title"],
            "spec_version": str(spec.document.get("info", {}).get("version", "")),
            "source": api["source"],
            "hosts": spec.hosts(api["hosts"]),
        }
        if api["id"] == "erp":
            entry["interactive"] = interactive_info(spec, interactive)
        apis[api["id"]] = entry
        for path, method, op in spec.operations():
            row = {
                "api": api["id"],
                "method": method.upper(),
                "path": path,
                "operation_id": op.get("operationId", ""),
                "summary": " ".join((op.get("summary") or "").split())[:160],
                "tags": sorted(op.get("tags", [])),
            }
            if method == "get":
                row["query"], row["required"] = spec.query_names(path, op)
            operations.append(row)
    return {
        "format": 2,
        "retrieved": retrieved,
        "note": "Derived by scripts/build_api_reference.py. Regenerate when a spec changes.",
        "apis": apis,
        "operations": operations,
    }


def build_salesorder_create_schema(spec, retrieved):
    op = spec.document["paths"]["/api/v3/SalesOrders"]["post"]
    return {
        "format": 1,
        "source": "Visma.net ERP SalesOrders API v3, POST /api/v3/SalesOrders request body",
        "retrieved": retrieved,
        "schema": spec.body_schema(op),
    }


def write_json(path, document):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(document, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n",
        encoding="utf-8",
    )
    print(f"wrote {path.relative_to(ROOT)} ({path.stat().st_size} bytes)")


def main():
    args = parse_args()
    specs = {
        "erp": Spec.load(args.erp),
        "salesorder": Spec.load(args.salesorder),
        "journaltx": Spec.load(args.journaltx),
    }
    interactive = Spec.load(args.erp_interactive)
    reference = build_reference(specs, interactive, args.retrieved)
    print("interactive ERP API has the same operations:", reference["apis"]["erp"]["interactive"]["same_operations"])
    write_json(REFERENCE_OUT, reference)
    write_json(SALESORDER_CREATE_OUT, build_salesorder_create_schema(specs["salesorder"], args.retrieved))


if __name__ == "__main__":
    main()
