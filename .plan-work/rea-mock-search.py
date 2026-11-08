import os
import re

SRC = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/apps/Frontend Plane/verevonv3/src"
os.chdir(SRC)

FACTORY_OBJ = "vi.mock('@/shared/api/spaces-client', () => ({"
FACTORY_IDENT = re.compile(r"vi\.mock\('@/shared/api/spaces-client', \(\) => ([A-Za-z_][A-Za-z0-9_]*)\)")
LINE = "  searchSpaceMessages: () => new Promise(() => {}),"

changed, skipped = [], []
for root, _, files in os.walk("."):
    for name in files:
        if not (name.endswith(".test.ts") or name.endswith(".test.tsx")):
            continue
        path = os.path.join(root, name)
        b = open(path, "rb").read().decode("utf-8")
        if "vi.mock('@/shared/api/spaces-client'" not in b or "searchSpaceMessages" in b:
            continue
        crlf = "\r\n" in b
        t = b.replace("\r\n", "\n")
        if FACTORY_OBJ in t:
            t = t.replace(FACTORY_OBJ, FACTORY_OBJ + "\n" + LINE, 1)
        else:
            m = FACTORY_IDENT.search(t)
            if not m:
                skipped.append(path)
                continue
            ident = m.group(1)
            hoist = "const " + ident + " = vi.hoisted(() => ({"
            if hoist in t:
                t = t.replace(hoist, hoist + "\n  searchSpaceMessages: vi.fn(() => new Promise(() => {})),", 1)
            else:
                skipped.append(path)
                continue
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
