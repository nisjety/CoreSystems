import pathlib
import sys

# Edits the scratchpad copies of ChatPanels.tsx and ChatPage.tsx for W2-FE-WS9H. Each anchor must match
# exactly once; the script stops on the first miss so a stale copy cannot be patched half way.
D = pathlib.Path(sys.argv[1])


def replace_once(name, old, new):
    path = D / name
    text = path.read_bytes().decode("utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{name}: expected 1 match, found {count} for anchor starting {old[:70]!r}")
    path.write_bytes(text.replace(old, new, 1).encode("utf-8"))
    print(f"ok {name}: {old[:60]!r}")


# ---- ChatPanels.tsx: header slot -------------------------------------------------------------------------
replace_once(
    "ChatPanels.tsx",
    "  onRegenerate: () => void\n}) {\n",
    "  onRegenerate: () => void\n"
    "  /** The chat browser's header control (WS9.H). Absent while the dock is off in this build. */\n"
    "  browserSlot?: JSX.Element\n"
    "}) {\n",
)
replace_once(
    "ChatPanels.tsx",
    '      <div class="verevon-chat-header__actions">\n        <Show when={workSurfaces().length > 0}>\n',
    '      <div class="verevon-chat-header__actions">\n        {props.browserSlot}\n        <Show when={workSurfaces().length > 0}>\n',
)

# ---- ChatPage.tsx: imports, dock body, class list, header slot, pane mount -------------------------------
replace_once(
    "ChatPage.tsx",
    "import { BrowserDockControls } from '@/features/browser/dock/BrowserDockControls'\n"
    "import {\n"
    "  browserDockSizeCss,\n"
    "  isChatBrowserDockEnabled,\n"
    "  nextBrowserDockPosition,\n"
    "  type BrowserDockVisiblePosition,\n"
    "} from '@/features/browser/dock/browser-dock-state'\n",
    "import { BrowserDockControls } from '@/features/browser/dock/BrowserDockControls'\n"
    "import { InteractiveBrowserPane } from '@/features/browser/dock/InteractiveBrowserPane'\n"
    "import { OpenBrowserButton } from '@/features/browser/dock/OpenBrowserButton'\n"
    "import { useDockBrowserSession } from '@/features/browser/dock/dock-session'\n"
    "import { createChatThreadByTab, planChatForTabSwitch } from '@/features/browser/dock/chat-per-tab'\n"
    "import {\n"
    "  browserDockPageClass,\n"
    "  browserDockSizeCss,\n"
    "  isChatBrowserDockEnabled,\n"
    "  nextBrowserDockPosition,\n"
    "  type BrowserDockVisiblePosition,\n"
    "} from '@/features/browser/dock/browser-dock-state'\n",
)

replace_once(
    "ChatPage.tsx",
    "  useChatShortcuts({\n    startNewChat,\n",
    "  // The browser body of the dock (WS9.H): the run watch, or the interactive browser beside the chat. It\n"
    "  // is kept for this page only; the layout itself stays in the dock record. Showing the browser needs no\n"
    "  // live run, so this body is what opens the dock on a chat without one.\n"
    "  const [dockBody, setDockBody] = createSignal<'run' | 'browser'>('run')\n"
    "  const browserBodyOpen = () => (\n"
    "    isChatBrowserDockEnabled() && dockBody() === 'browser' && browserDock().position !== 'hidden'\n"
    "  )\n"
    "  // The browser body shows only on the chat tab. A canvas tab owns the rail, as it does for the run watch.\n"
    "  const browserBodyShown = () => browserBodyOpen() && activeTab() === 'chat'\n"
    "  // Hiding the dock returns the rail to the run watch, so restoring it shows the run again.\n"
    "  createEffect(\n"
    "    () => browserDock().position === 'hidden',\n"
    "    (hidden) => {\n"
    "      if (hidden) setDockBody('run')\n"
    "    },\n"
    "  )\n"
    "  const openBrowserBody = () => {\n"
    "    if (browserDock().position === 'hidden') toggleRunPanel()\n"
    "    setDockBody('browser')\n"
    "  }\n"
    "  const closeBrowserBody = () => {\n"
    "    setDockBody('run')\n"
    "    if (!liveRunId()) setBrowserDockPosition('hidden')\n"
    "  }\n"
    "  const toggleBrowserBody = () => {\n"
    "    if (browserBodyOpen()) closeBrowserBody()\n"
    "    else openBrowserBody()\n"
    "  }\n"
    "  // The layout class of the browser body. The run watch's own class still comes from dockLayout.\n"
    "  const browserBodyDock = () => (browserBodyOpen() ? browserDockPageClass(browserDock().position) : null) ?? ''\n"
    "\n"
    "  // One interactive browser session per chat page (WS9.H). Its zero retention follows the thread on\n"
    "  // screen and the workspace's retention verdict.\n"
    "  const dockSession = useDockBrowserSession({\n"
    "    orgId: () => session.activeOrg?.id,\n"
    "    threadTemporary: () => isActiveThreadTemporary(),\n"
    "    tr: i18n.tr,\n"
    "  })\n"
    "\n"
    "  // Chat per browser tab (WS9.H). The rule comes from the dock record: `keep` unless the record says `new`.\n"
    "  // A chat moves only when the browser's active tab changes, and never without a browser session.\n"
    "  const chatThreadsByTab = createChatThreadByTab()\n"
    "  createEffect(\n"
    "    () => dockSession.controller?.preview()?.browserSession?.session.tabs?.find((tab) => tab.active)?.tabId ?? null,\n"
    "    (tabKey, previousKey) => {\n"
    "      if (previousKey && state.threadId) chatThreadsByTab.remember(previousKey, state.threadId)\n"
    "      const plan = planChatForTabSwitch({\n"
    "        previousTab: previousKey ?? null,\n"
    "        nextTab: tabKey,\n"
    "        rule: browserDock().chatPerTab,\n"
    "        threadFor: chatThreadsByTab.threadFor,\n"
    "      })\n"
    "      if (plan.kind === 'new-thread') startNewChat()\n"
    "      else if (plan.kind === 'select-thread') selectChatThread(plan.threadId)\n"
    "    },\n"
    "  )\n"
    "\n"
    "  // The composer, found the way the focus-composer shortcut finds it.\n"
    "  const focusComposer = () => {\n"
    "    document.querySelector<HTMLTextAreaElement>('.verevon-dashboard-textarea')?.focus()\n"
    "  }\n"
    "  // The omnibox's \"Ask Verevon\" row: the typed text becomes the message and the composer takes focus.\n"
    "  const askVerevonFromBrowser = (text: string) => {\n"
    "    setInput(text)\n"
    "    focusComposer()\n"
    "  }\n"
    "\n"
    "  useChatShortcuts({\n"
    "    startNewChat,\n",
)

replace_once(
    "ChatPage.tsx",
    "          'verevon-chat-page--dock-maximised': dockLayout() === 'maximised',\n"
    "        },\n"
    "      ]}\n",
    "          'verevon-chat-page--dock-maximised': dockLayout() === 'maximised',\n"
    "        },\n"
    "        browserBodyDock(),\n"
    "      ]}\n",
)

replace_once(
    "ChatPage.tsx",
    "            onNewChat={startNewChat}\n",
    "            browserSlot={browserHeaderButton()}\n"
    "            onNewChat={startNewChat}\n",
)

replace_once(
    "ChatPage.tsx",
    "        hidden={activeTab() !== 'chat' && !workCanvasActive()}\n",
    "        hidden={(activeTab() !== 'chat' && !workCanvasActive()) || browserBodyShown()}\n",
)

replace_once(
    "ChatPage.tsx",
    "        dockControls={liveRailDockControls()}\n"
    "      />\n"
    "      <Show when={scheduleDialogOpen()}>\n",
    "        dockControls={liveRailDockControls()}\n"
    "      />\n"
    "      {/* The interactive browser body (WS9.H) takes the rail while the browser is open beside the chat. */}\n"
    "      <Show when={browserBodyOpen() ? dockSession.controller : null}>\n"
    "        {(controller) => (\n"
    "          <InteractiveBrowserPane\n"
    "            controller={controller()}\n"
    "            orgId={() => session.activeOrg?.id}\n"
    "            zdr={dockSession.zdr}\n"
    "            dockPosition={browserDock().position}\n"
    "            onDockPositionChange={setBrowserDockPosition}\n"
    "            sizeCss={browserDockSizeCss(browserDock())}\n"
    "            hidden={activeTab() !== 'chat'}\n"
    "            onAskVerevon={askVerevonFromBrowser}\n"
    "            onAskAboutPage={focusComposer}\n"
    "            onClose={closeBrowserBody}\n"
    "          />\n"
    "        )}\n"
    "      </Show>\n"
    "      <Show when={scheduleDialogOpen()}>\n",
)

replace_once(
    "ChatPage.tsx",
    "      ? <BrowserDockControls position={browserDock().position} onPositionChange={setBrowserDockPosition} />\n"
    "      : undefined\n"
    "  ))\n",
    "      ? <BrowserDockControls position={browserDock().position} onPositionChange={setBrowserDockPosition} />\n"
    "      : undefined\n"
    "  ))\n"
    "  // The browser header control (WS9.H), built once. Absent while the dock is off in this build.\n"
    "  const browserHeaderButton = createMemo(() => (\n"
    "    isChatBrowserDockEnabled()\n"
    "      ? <OpenBrowserButton open={browserBodyOpen} onToggle={toggleBrowserBody} />\n"
    "      : undefined\n"
    "  ))\n",
)

print("all anchors matched once")
