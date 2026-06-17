// @vitest-environment jsdom

import { render, screen } from '@solidjs/testing-library'
import { describe, expect, it } from 'vitest'
import { VelionIconButton } from '@/shared/ui/velion/VelionIconButton'

describe('VelionIconButton', () => {
  it('supports shared primary tone and compact sizing without page-specific overrides', () => {
    render(() => (
      <VelionIconButton
        aria-label="Primary action"
        shape="rounded"
        size="xs"
        tone="primary"
      >
        Go
      </VelionIconButton>
    ))

    const button = screen.getByRole('button', { name: 'Primary action' })
    expect(button.className).toContain('velion-icon-button--primary')
    expect(button.className).toContain('velion-icon-button--xs')
    expect(button.className).toContain('velion-icon-button--rounded')
  })
})
