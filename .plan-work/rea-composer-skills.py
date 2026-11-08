import os

BASE = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/apps/Frontend Plane/verevonv3/src/features/spaces/components"
os.chdir(BASE)


def edit(path, pairs):
    b = open(path, "rb").read().decode("utf-8")
    crlf = "\r\n" in b
    t = b.replace("\r\n", "\n")
    for old, new in pairs:
        assert t.count(old) == 1, (path, t.count(old), old[:80])
        t = t.replace(old, new)
    if crlf:
        t = t.replace("\n", "\r\n")
    open(path, "wb").write(t.encode("utf-8"))


# Code: the skill status line shows whenever no skill is visible, not only when the whole picker is
# empty. With the platform commands listed (WS13-S7) a failed or loading catalogue otherwise says nothing.
edit("SpaceRoomComposer.tsx", [(
    "            <Show when={skillQuery() !== undefined && pickerItems().length === 0}>",
    "            <Show when={skillQuery() !== undefined && visibleSkills().length === 0}>",
)])

edit("SpaceRoomComposer.test.tsx", [
    (
        """    const options = await screen.findAllByRole('option')
    expect(options.map((option) => option.textContent)).toEqual([
      'HMS-rundeOrganisasjon',
      'InnkjøpsrutineOrganisasjon',
      'Min sjekklistePersonlig',
    ])
""",
        """    // The platform commands list first (WS13-S7); the skills follow once their catalogue has loaded.
    await screen.findByRole('option', { name: /HMS-runde/ })
    const labels = screen.getAllByRole('option').map((option) => option.textContent ?? '')
    expect(labels).toEqual(
      expect.arrayContaining(['HMS-rundeOrganisasjon', 'InnkjøpsrutineOrganisasjon', 'Min sjekklistePersonlig']),
    )
""",
    ),
    (
        """    fireEvent.keyDown(textarea(), { key: 'ArrowDown' })
    flush()
    expect(screen.getByRole('option', { name: /Innkjøpsrutine/ }).getAttribute('aria-selected')).toBe('true')
""",
        """    fireEvent.keyDown(textarea(), { key: 'ArrowDown' })
    flush()
    expect(screen.getAllByRole('option')[1]?.getAttribute('aria-selected')).toBe('true')
""",
    ),
    (
        """    expect(await screen.findByText(/kunne ikke hentes/)).toBeTruthy()
    expect(screen.queryByRole('option')).toBeNull()
""",
        """    expect(await screen.findByText(/kunne ikke hentes/)).toBeTruthy()
    expect(screen.queryByRole('option', { name: /Innkjøpsrutine|HMS-runde|sjekkliste/ })).toBeNull()
""",
    ),
])
print("composer skill fixes applied")
