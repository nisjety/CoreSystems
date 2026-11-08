import os

SRC = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/apps/Frontend Plane/verevonv3/src"
path = os.path.join(SRC, "features/core/components/sidebar/SpaceAttentionBadge.tsx")
b = open(path, "rb").read().decode("utf-8")
crlf = "\r\n" in b
t = b.replace("\r\n", "\n")


def sub(old, new):
    global t
    assert t.count(old) == 1, (t.count(old), old[:80])
    t = t.replace(old, new)


sub(
    "import { Show, createSignal, onCleanup } from 'solid-js'\n",
    "import { Show, createSignal, onSettled } from 'solid-js'\n",
)
sub(
    """export function useSpaceAttentionPoller(): void {
  const i18n = useI18n()
  onCleanup(retainSpaceAttention(i18n.tr))
}
""",
    """export function useSpaceAttentionPoller(): void {
  const i18n = useI18n()
  // Joined once settled, not in the component body: starting the poller writes signals (its clock and
  // its first state), and Solid 2 refuses a write inside a component body (REACTIVE_WRITE_IN_OWNED_SCOPE).
  // The returned release is the cleanup.
  onSettled(() => retainSpaceAttention(i18n.tr))
}
""",
)

if crlf:
    t = t.replace("\n", "\r\n")
open(path, "wb").write(t.encode("utf-8"))
print("attention poller joins once settled")
