// @vitest-environment jsdom

import { describe, expect, it } from 'vitest'

import { defaultSidebarExpandedForRoute } from './CoreShell'

describe('the sidebar default on a first visit', () => {
  it('starts expanded on the rooms index and on an open room at desktop width', () => {
    expect(defaultSidebarExpandedForRoute('/spaces', { pathname: '/spaces', phone: false })).toBe(true)
    expect(defaultSidebarExpandedForRoute('/spaces', { pathname: '/spaces/space_room_1', phone: false })).toBe(true)
    expect(defaultSidebarExpandedForRoute('/spaces')).toBe(true)
  })

  it('starts closed over an open room at phone width, so the room is what the person sees', () => {
    expect(defaultSidebarExpandedForRoute('/spaces', { pathname: '/spaces/space_room_1', phone: true })).toBe(false)
    expect(defaultSidebarExpandedForRoute('/spaces', { pathname: '/spaces/space_room_1/', phone: true })).toBe(false)
  })

  it('keeps the rooms list open at phone width, where the list is the page', () => {
    expect(defaultSidebarExpandedForRoute('/spaces', { pathname: '/spaces', phone: true })).toBe(true)
    expect(defaultSidebarExpandedForRoute('/spaces', { pathname: '/spaces/', phone: true })).toBe(true)
  })

  it('does not change any other route', () => {
    expect(defaultSidebarExpandedForRoute('/settings', { pathname: '/settings', phone: true })).toBe(true)
    expect(defaultSidebarExpandedForRoute('/chat', { pathname: '/chat', phone: true })).toBe(false)
  })
})
