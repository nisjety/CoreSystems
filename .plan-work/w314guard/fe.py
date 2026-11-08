import sys
sys.path.insert(0, r'C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b\.plan-work\w314guard')
from edit import edit

# EP-2-S4: chat-normalizers.ts
edit('features/chat/components/chat-normalizers.ts', [
("""  type ChatAction,
  type ChatMessage,
  getAttachmentPolicy,
  type RecalledMemory,
} from '@/shared/api/chat-client'
""", """  type ChatAction,
  type ChatMessage,
  type CitationLocator,
  getAttachmentPolicy,
  type RecalledMemory,
  type ResearchProvenance,
} from '@/shared/api/chat-client'
""", 1),
("""export function normalizeArtifact(event: { id?: string; kind?: string; title?: string; content?: string; version?: number }): ChatArtifact | null {""",
 """export function normalizeArtifact(event: { id?: string; kind?: string; title?: string; content?: string; version?: number; provenance?: ResearchProvenance }): ChatArtifact | null {""", 1),
("""    content,
    version: event.version ?? 0,
  }
}
""", """    content,
    version: event.version ?? 0,
    ...(event.provenance ? { provenance: event.provenance } : {}),
  }
}
""", 1),
("""start?: number; end?: number; origin?: string }): Citation | null {""",
 """start?: number; end?: number; origin?: string; quote?: string; locator?: CitationLocator }): Citation | null {""", 1),
("""    ...(event.end !== undefined ? { end: event.end } : {}),
    ...(origin ? { origin } : {}),
  }
}
""", """    ...(event.end !== undefined ? { end: event.end } : {}),
    ...(origin ? { origin } : {}),
    ...(event.quote ? { quote: event.quote } : {}),
    ...(event.locator ? { locator: event.locator } : {}),
  }
}
""", 1),
])

# EP-2-S4: ChatArtifactPanel.tsx
edit('features/chat/components/ChatArtifactPanel.tsx', [
("""import { officePdfRenderer } from './ArtifactOfficePreview'
""", """import { officePdfRenderer } from './ArtifactOfficePreview'
import { ArtifactProvenance } from './ArtifactProvenance'
""", 1),
("""          </Switch>
          </Show>
        </Show>
        </Show>
      </div>
    </section>
""", """          </Switch>
          </Show>
        </Show>
        </Show>
      </div>
      <ArtifactProvenance provenance={(artifact() as { provenance?: unknown }).provenance} />
    </section>
""", 1),
])

# EP-2-S4, EP-2-S8, EP-10-S1: model-access.ts
edit('shared/actions/model-access.ts', [
("""  'approvals.decide': exempt('irreversible_destructive', 'destructive'),
""", """  'approvals.decide': exempt('irreversible_destructive', 'destructive'),
  'approvals.preview': exempt('approval_authority', 'read'),
""", 1),
("""  'chat.inspect_run_request': exempt('no_owner_executor', 'read'),
""", """  'chat.inspect_run_request': exempt('no_owner_executor', 'read'),
  'chat.open_cited_passage': exempt('no_owner_executor', 'read'),
  'chat.wide_research_start': exempt('no_owner_executor', 'external_write'),
""", 1),
])

# EP-6-S3
edit('shared/api/cron-client.ts', [
("""  withheld_effects: number
}
""", """  withheld_effects: number
  /** The fire's key: the schedule slot's time, or manual:<request id> for a Run now (EP-6-S3). */
  fire_key?: string
}
""", 1),
])
edit('features/agents/components/AgentsHubSummary.tsx', [
("automationStateOf(schedule, null, now)", "automationStateOf(schedule, now)", 1),
])

# EP-1-S2
edit('shared/api/orchestration-client.ts', [
("""    stepUpRequired: cause.step_up_required === true,
  }
}
""", """    stepUpRequired: cause.step_up_required === true,
    assistKind: str(cause.assist_kind),
    assistOrigin: str(cause.assist_origin),
    host: str(cause.host),
  }
}
""", 1),
])

# EP-2-S4 + EP-1-S3: global.css
edit('styles/global.css', [
("""  font-size: 11px;
}

.verevon-chat-run-panel__body {
""", """  font-size: 11px;
}

.verevon-chat-run-panel__give-back-note {
  display: grid;
  gap: 4px;
  margin-top: 8px;
  font-size: 12px;
}

.verevon-chat-run-panel__give-back-note textarea {
  width: 100%;
  resize: vertical;
  font: inherit;
}

.verevon-chat-run-panel__body {
""", 1),
(""".verevon-chat-citation-chip__group-label {
  margin: 0 0 2px;
  padding: 0 6px;
  color: var(--verevon-e-muted);
  font-size: 9px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.03em;
}
""", """.verevon-chat-citation-chip__group-label {
  margin: 0 0 2px;
  padding: 0 6px;
  color: var(--verevon-e-muted);
  font-size: 9px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.03em;
}

/* EP-2-S4: the passage a cited source contributed, in a side sheet and on the source card. */
.verevon-chat-cited-passage {
  position: fixed;
  inset: 0 0 0 auto;
  width: min(420px, 100vw);
  z-index: 60;
  overflow: auto;
  padding: 16px;
  background: var(--surface, #fff);
  box-shadow: -8px 0 24px rgb(0 0 0 / 0.12);
}

.verevon-chat-cited-passage__header {
  display: flex;
  justify-content: space-between;
  gap: 8px;
}

.verevon-chat-cited-passage__quote,
.verevon-chat-source-card__quote {
  margin: 8px 0;
  padding-left: 10px;
  border-left: 3px solid currentColor;
  opacity: 0.85;
}

.verevon-chat-citation-chip__passage,
.verevon-chat-source-card__passage {
  font-size: 12px;
}
""", 1),
])
