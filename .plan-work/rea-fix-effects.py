import os

os.chdir("C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/apps/Frontend Plane/verevonv3/src/features/spaces/components")
p = "SpaceRoomComposer.tsx"
b = open(p, "rb").read().decode("utf-8")
crlf = "\r\n" in b
b = b.replace("\r\n", "\n")


def sub(old, new):
    global b
    assert b.count(old) == 1, (b.count(old), old[:90])
    b = b.replace(old, new)


# 1. mention rebind: the replied-to thread is the tracked value
sub(
    """  createEffect(() => {
    mention.rebind(props.replyTarget?.()?.threadId)
  })
""",
    """  createEffect(
    () => props.replyTarget?.()?.threadId,
    (threadId) => {
      mention.rebind(threadId)
    },
  )
""",
)

# 2. grow the box: every signal it depends on is read in the compute function
sub(
    """  createEffect(() => {
    text()
    previewing()
    const element = textareaRef
    if (!element) return
    if (!markdownOn()) {
      element.rows = MARKDOWN_ROWS_MIN
      return
    }
    element.rows = MARKDOWN_ROWS_MIN
    const lineHeight = Number.parseFloat(getComputedStyle(element).lineHeight)
    if (!Number.isFinite(lineHeight) || lineHeight <= 0 || element.scrollHeight <= 0) return
    const style = getComputedStyle(element)
    const padding = (Number.parseFloat(style.paddingTop) || 0) + (Number.parseFloat(style.paddingBottom) || 0)
    const needed = Math.ceil((element.scrollHeight - padding) / lineHeight)
    element.rows = Math.max(MARKDOWN_ROWS_MIN, Math.min(MARKDOWN_ROWS_MAX, needed))
  })
""",
    """  createEffect(
    () => ({ text: text(), previewing: previewing(), markdown: markdownOn() }),
    () => {
      const element = textareaRef
      if (!element) return
      if (!markdownOn()) {
        element.rows = MARKDOWN_ROWS_MIN
        return
      }
      element.rows = MARKDOWN_ROWS_MIN
      const lineHeight = Number.parseFloat(getComputedStyle(element).lineHeight)
      if (!Number.isFinite(lineHeight) || lineHeight <= 0 || element.scrollHeight <= 0) return
      const style = getComputedStyle(element)
      const padding = (Number.parseFloat(style.paddingTop) || 0) + (Number.parseFloat(style.paddingBottom) || 0)
      const needed = Math.ceil((element.scrollHeight - padding) / lineHeight)
      element.rows = Math.max(MARKDOWN_ROWS_MIN, Math.min(MARKDOWN_ROWS_MAX, needed))
    },
  )
""",
)

# 3. an emptied box leaves the preview
sub(
    """  createEffect(() => {
    if (previewing() && (!markdownOn() || text().trim() === '')) setPreviewing(false)
  })
""",
    """  createEffect(
    () => previewing() && (!markdownOn() || text().trim() === ''),
    (leavePreview) => {
      if (leavePreview) setPreviewing(false)
    },
  )
""",
)

# 4. a waiting message goes out once the room is idle: the reads are in the compute function
sub(
    """  createEffect(() => {
    const waiting = midRun.deferred()
    if (!waiting || submitting() || !isSpaceObserved(props.spaceRef)) return
    if (liveThreadsIn(props.spaceRef).some((thread) => thread.thread_id === waiting.threadId)) return
    const ready = midRun.takeDeferred()
""",
    """  createEffect(
    () => ({
      waiting: midRun.deferred(),
      busy: submitting(),
      observed: isSpaceObserved(props.spaceRef),
      threads: liveThreadsIn(props.spaceRef),
    }),
    ({ waiting, busy, observed, threads }) => {
    if (!waiting || busy || !observed) return
    if (threads.some((thread) => thread.thread_id === waiting.threadId)) return
    const ready = midRun.takeDeferred()
""",
)

if crlf:
    b = b.replace("\n", "\r\n")
open(p, "wb").write(b.encode("utf-8"))
print("composer effects converted")
