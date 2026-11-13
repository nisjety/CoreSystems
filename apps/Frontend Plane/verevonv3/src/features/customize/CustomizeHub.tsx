import { Match, Show, Switch } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { useI18n } from '@/shared/i18n'
import { SettingsHero, SettingsSurface } from '@/features/settings/components/settings-ui'
import { McpServersSection } from '@/features/settings/components/McpServersSection'
import { OrgInstructionsSection } from '@/features/settings/components/OrgInstructionsSection'
import { PluginsSection } from '@/features/settings/components/PluginsSection'
import { SkillsSection } from '@/features/settings/components/SkillsSection'
import { SkillLibrary } from '@/features/skills/components/SkillLibrary'
import { AgentModesSection, orgAgentPolicyAvailable } from '@/features/customize/AgentModesSection'
import { HooksSection } from '@/features/customize/HooksSection'
import { PermissionRulesSection } from '@/features/customize/PermissionRulesSection'
import { OrgWebAccessSection } from '@/features/customize/OrgWebAccessSection'

/**
 * The Customize hub (WS25-S11): where an organisation shapes what its agents may do. It composes the existing
 * MCP, skills, organisation-instruction and plugin sections with the agent-mode and hook sections, and it sits
 * behind ORG_AGENT_POLICY_API. While the deployment has that API off, the hub says so and offers no editor, so
 * nothing in it can look configurable when nothing enforces it.
 */
/** EP-11 S9: the Space-scoped skill library, off unless the build sets VITE_SKILL_LIBRARY to the literal `true`. */
const skillLibraryEnabled = () => import.meta.env.VITE_SKILL_LIBRARY === 'true'
/** W5-074-c: the organisation permission rules, off unless the build sets VITE_PERMISSION_RULES to the literal `true`. */
const permissionRulesEnabled = () => import.meta.env.VITE_PERMISSION_RULES === 'true'

export default function CustomizeHub() {
  const i18n = useI18n()
  const [available] = createResource(() => orgAgentPolicyAvailable())

  return (
    <SettingsSurface>
      <SettingsHero
        eyebrow={i18n.tr('Agenter', 'Agents')}
        title={i18n.tr('Tilpass', 'Customize')}
        description={i18n.tr(
          'Hva agentene i organisasjonen kan gjøre, hvilke verktøy og instrukser de bruker, og hvilke regler som gjelder før de handler.',
          'What the agents in your organisation may do, which tools and instructions they use, and which rules apply before they act.',
        )}
      />
      <Switch>
        <Match when={available.loading}>
          <p class="verevon-settings-subnote" role="status" aria-busy="true">
            {i18n.tr('Laster…', 'Loading…')}
          </p>
        </Match>
        <Match when={available.error}>
          <p class="verevon-settings-status-message verevon-settings-status-message--error" role="alert">
            {i18n.tr('Kunne ikke laste Tilpass.', 'Could not load Customize.')}
          </p>
        </Match>
        <Match when={available() === false}>
          <p class="verevon-settings-subnote">
            {i18n.tr(
              'Tilpass er ikke slått på for denne installasjonen ennå.',
              'Customize is not switched on for this deployment yet.',
            )}
          </p>
        </Match>
        <Match when={available() === true}>
          <AgentModesSection />
          <Show when={permissionRulesEnabled()}>
            <PermissionRulesSection />
          </Show>
          <OrgWebAccessSection />
          <HooksSection />
          <McpServersSection />
          <SkillsSection />
          <Show when={skillLibraryEnabled()}>
            <SkillLibrary />
          </Show>
          <OrgInstructionsSection />
          <PluginsSection />
        </Match>
      </Switch>
    </SettingsSurface>
  )
}
