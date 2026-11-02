// WS04-S6: the SPA mirror of Control's room role rules, checked against the vendored golden
// `contracts/space-room-role-matrix.v1.json` (the Go side copies it byte for byte, see
// scripts/tests/space-contract-vectors-sync.test.sh). The mirror only decides which controls to
// offer; Control decides every call.

import { describe, expect, it } from 'vitest'

import matrix from '@/shared/api/contracts/space-room-role-matrix.v1.json'

import {
  assignableRolesForAdd,
  assignableRolesForChange,
  canAdd,
  canChangeRole,
  canJoin,
  canRemove,
  countActiveOwners,
  isElevatedRole,
  isRoomRole,
  roleRank,
  ROOM_ROLES,
} from './space-roles'

type Denial = string | null

describe('space-room-role-matrix.v1.json', () => {
  it('has the shape the mirror is checked with', () => {
    expect(matrix.version).toBe(1)
    expect(matrix.ranks).toEqual({ viewer: 1, editor: 2, manager: 3, owner: 4 })
    expect(matrix.elevated_min_rank).toBe(3)
    expect(matrix.vectors.add.length).toBeGreaterThan(0)
    expect(matrix.vectors.change_role.length).toBeGreaterThan(0)
    expect(matrix.vectors.remove.length).toBeGreaterThan(0)
    expect(matrix.vectors.join.length).toBeGreaterThan(0)
  })

  it('agrees with the mirror on every rank and on the elevated floor', () => {
    for (const [role, rank] of Object.entries(matrix.ranks)) {
      expect(roleRank(role)).toBe(rank)
      expect(isElevatedRole(role)).toBe(rank >= matrix.elevated_min_rank)
    }
    expect(ROOM_ROLES).toEqual(['viewer', 'editor', 'manager', 'owner'])
  })

  it('canAdd matches every add vector', () => {
    for (const vector of matrix.vectors.add) {
      expect(canAdd(vector.actor, vector.new_role), JSON.stringify(vector)).toBe(vector.denial as Denial)
    }
  })

  it('canChangeRole matches every change_role vector', () => {
    for (const vector of matrix.vectors.change_role) {
      expect(
        canChangeRole(vector.actor, vector.target, vector.new_role, vector.target_is_actor, vector.active_owners),
        JSON.stringify(vector),
      ).toBe(vector.denial as Denial)
    }
  })

  it('canRemove matches every remove vector', () => {
    for (const vector of matrix.vectors.remove) {
      expect(
        canRemove(vector.actor, vector.target, vector.target_is_actor, vector.active_owners),
        JSON.stringify(vector),
      ).toBe(vector.denial as Denial)
    }
  })

  it('canJoin matches every join vector', () => {
    for (const vector of matrix.vectors.join) {
      expect(canJoin(vector.join_role), JSON.stringify(vector)).toBe(vector.denial as Denial)
    }
  })
})

describe('role helpers', () => {
  it('knows a room role from any other string', () => {
    expect(isRoomRole('manager')).toBe(true)
    expect(isRoomRole('admin')).toBe(false)
    expect(isRoomRole('toString')).toBe(false)
    expect(isRoomRole(undefined)).toBe(false)
    expect(roleRank('toString')).toBe(0)
    expect(roleRank('')).toBe(0)
  })

  it('offers an add role only up to what the actor may grant', () => {
    expect(assignableRolesForAdd('viewer')).toEqual([])
    expect(assignableRolesForAdd('editor')).toEqual([])
    expect(assignableRolesForAdd('manager')).toEqual(['viewer', 'editor'])
    expect(assignableRolesForAdd('owner')).toEqual(['viewer', 'editor', 'manager', 'owner'])
    expect(assignableRolesForAdd('')).toEqual([])
  })

  it('lists the change roles the matrix allows, always including the current one', () => {
    // A manager cannot touch an elevated member, but the current role is still shown.
    expect(assignableRolesForChange('manager', 'owner', false, 2)).toEqual(['owner'])
    expect(assignableRolesForChange('manager', 'editor', false, 1)).toEqual(['viewer', 'editor'])
    expect(assignableRolesForChange('owner', 'editor', false, 1)).toEqual(['viewer', 'editor', 'manager', 'owner'])
    // The sole owner cannot demote themselves; a second owner can.
    expect(assignableRolesForChange('owner', 'owner', true, 1)).toEqual(['owner'])
    expect(assignableRolesForChange('owner', 'owner', true, 2)).toEqual(['viewer', 'editor', 'manager', 'owner'])
    // Nobody promotes themselves.
    expect(assignableRolesForChange('editor', 'editor', true, 1)).toEqual(['viewer', 'editor'])
  })

  it('counts only active human owners', () => {
    expect(
      countActiveOwners([
        { subject_type: 'user', role: 'owner' },
        { subject_type: 'user', role: 'editor' },
        { subject_type: 'service', role: 'owner' },
        { subject_type: 'user', role: 'owner' },
      ]),
    ).toBe(2)
    expect(countActiveOwners([])).toBe(0)
  })
})
