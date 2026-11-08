import os

BASE = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/apps/Frontend Plane/verevonv3/src/features/spaces/components"
os.chdir(BASE)
path = "SpaceRoomComposer.test.tsx"
b = open(path, "rb").read().decode("utf-8")
crlf = "\r\n" in b
t = b.replace("\r\n", "\n")
old = """    // No "this room" badge exists to give: the registry has no Space scope.
    expect(screen.queryByText(/rommet/i)).toBeNull()
"""
new = """    // No "this room" badge exists to give: the registry has no Space scope. The platform command hints
    // legitimately say "rommet", so only the skill rows are checked.
    const skillRows = screen
      .getAllByRole('option')
      .filter((option) => /HMS-runde|Innkjøpsrutine|sjekkliste/.test(option.textContent ?? ''))
    expect(skillRows).toHaveLength(3)
    for (const row of skillRows) expect(row.textContent).not.toMatch(/rommet/i)
"""
assert t.count(old) == 1, t.count(old)
t = t.replace(old, new)
if crlf:
    t = t.replace("\n", "\r\n")
open(path, "wb").write(t.encode("utf-8"))
print("scoped")
