import os

os.chdir("C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/apps/Frontend Plane/verevonv3/src/features/spaces/components")
p = "SpaceRoomComposer.tsx"
b = open(p, "rb").read().decode("utf-8")
crlf = "\r\n" in b
b = b.replace("\r\n", "\n")

OPEN = "  createEffect(\n    () => ({\n      waiting: midRun.deferred(),"
assert b.count(OPEN) == 1, b.count(OPEN)
start = b.index(OPEN)
CLOSE = "\n    })\n  })\n\n  const candidates"
close_at = b.index(CLOSE, start)
block = b[start:close_at]
lines = block.split("\n")
head_end = lines.index("    ({ waiting, busy, observed, threads }) => {")
compute = lines[1:head_end]          # the compute object, already at 4..6 spaces
body = lines[head_end + 1:]          # guard lines and the rest of the effect body, at 4 spaces
body = [("  " + line) if line.strip() else line for line in body]
new_block = "\n".join(
    ["  createEffect("]
    + compute
    + ["    ({ waiting, busy, observed, threads }) => {"]
    + body
)
new_block += "\n      })\n    },\n  )"
b = b[:start] + new_block + b[close_at + len("\n    })\n  })"):]

if crlf:
    b = b.replace("\n", "\r\n")
open(p, "wb").write(b.encode("utf-8"))
print("re-indented")
