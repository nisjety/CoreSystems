// Better Auth admin access-control roles.
//
// Better Auth >= 1.6 validates that every role listed in the admin plugin's
// `adminRoles` is also defined in its `roles` map (otherwise it throws
// "Invalid admin roles: <role>. Admin roles must be defined in the 'roles'
// configuration." at startup). We treat `superadmin` as an admin role
// (see ADMIN_ROLES default + orpc-router `adminRoleNames`), so it must be a
// defined role here.
//
// Roles are built on the admin plugin's DEFAULT statements so the built-in
// admin operations (ban, set-role, list users, list/revoke sessions, etc.)
// authorize correctly. `admin` and `superadmin` both receive the full default
// admin permission set; regular `user` has no admin-plugin permissions.
import { createAccessControl } from 'better-auth/plugins/access';
import { adminAc, defaultStatements } from 'better-auth/plugins/admin/access';

export const ac = createAccessControl(defaultStatements);

export const roles = {
  // Regular users hold no admin-plugin permissions.
  user: ac.newRole({}),
  // Default admin permission set.
  admin: ac.newRole({ ...adminAc.statements }),
  // Superadmin: full admin permissions (distinct label, same authority).
  superadmin: ac.newRole({ ...adminAc.statements }),
};
