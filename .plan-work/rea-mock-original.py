import os
import re

SRC = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/apps/Frontend Plane/verevonv3/src"
os.chdir(SRC)

HEAD_OBJ = "vi.mock('@/shared/api/spaces-client', () => ({"
HEAD_NEW = (
    "vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({\n"
    "  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),"
)
IDENT = re.compile(r"vi\.mock\('@/shared/api/spaces-client', \(\) => ([A-Za-z_][A-Za-z0-9_]*)\)")

changed, skipped = [], []
for root, _, files in os.walk("."):
    for name in files:
        if not (name.endswith(".test.ts") or name.endswith(".test.tsx")):
            continue
        path = os.path.join(root, name)
        b = open(path, "rb").read().decode("utf-8")
        if "vi.mock('@/shared/api/spaces-client'" not in b:
            continue
        if "importOriginal<typeof import('@/shared/api/spaces-client')>" in b:
            continue
        crlf = "\r\n" in b
        t = b.replace("\r\n", "\n")
        if HEAD_OBJ in t:
            t = t.replace(HEAD_OBJ, HEAD_NEW, 1)
        else:
            m = IDENT.search(t)
            if not m:
                skipped.append(path)
                continue
            ident = m.group(1)
            replacement = (
                "vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({\n"
                "  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),\n"
                "  ..." + ident + ",\n"
                "}))"
            )
            t = t[:m.start()] + replacement + t[m.end():]
        if crlf:
            t = t.replace("\n", "\r\n")
        open(path, "wb").write(t.encode("utf-8"))
        changed.append(path)

print("changed", len(changed))
for p in changed:
    print("  ", p)
print("skipped", len(skipped))
for p in skipped:
    print("  SKIP", p)
