// @vitest-environment jsdom

import { render, screen } from '@solidjs/testing-library'
import { describe, expect, it } from 'vitest'
import { VerevonIconButton } from '@/shared/ui/verevon/VerevonIconButton'

describe('VerevonIconButton', () => {
  it('supports shared primary tone and compact sizing without page-specific overrides', () => {
    render(() => (
      <VerevonIconButton
        aria-label="Primary action"
        shape="rounded"
        size="xs"
        tone="primary"
      >
        Go
      </VerevonIconButton>
    ))

    const button = screen.getByRole('button', { name: 'Primary action' })
    expect(button.className).toContain('verevon-icon-button--primary')
    expect(button.className).toContain('verevon-icon-button--xs')
    expect(button.className).toContain('verevon-icon-button--rounded')
  })
})
