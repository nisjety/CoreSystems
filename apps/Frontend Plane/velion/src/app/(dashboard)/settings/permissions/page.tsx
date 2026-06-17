import { SettingsSectionFrame } from '@/components/settings/SettingsSectionFrame'
import { PermissionsEditor } from '@/components/settings/permissions/PermissionsEditor'

// U6-3 (ui-ux-velion-gap.md §10): real RBAC editor.
//
// Replaces the previous <SettingsPlaceholderPanel> with a live editor backed
// by org-core's role/permission surface (see internal/rbac/ in org-core).
// The editor lets workspace owners + admins:
//   - browse all roles (default 4 + custom)
//   - edit each role's permissions against the global capability catalog
//   - create new custom roles
//   - delete custom roles
//
// Member-role assignment is exposed via the members table (server-rendered
// in the editor) — picking a different role for a member calls
// PATCH /orgs/:id/members/:userId/role.

export default function SettingsPermissionsPage() {
  return (
    <SettingsSectionFrame
      eyebrow="Access & control"
      title="Permissions"
      description="Define role boundaries and workspace-level access policies. Each role is a named bundle of capabilities; assign roles to members on the members tab."
    >
      <PermissionsEditor />
    </SettingsSectionFrame>
  )
}
