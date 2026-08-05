// @vitest-environment jsdom

import { render, screen } from '@solidjs/testing-library'
import { describe, expect, it } from 'vitest'
import { Button } from '@/shared/ui/Button'
import { buttonClasses } from '@/shared/ui/button-classes'

describe('Button', () => {
  it('uses standardized variant, size, and width classes from props instead of page-level overrides', () => {
    render(() => (
      <Button variant="primary" size="lg" fullWidth>
        Continue
      </Button>
    ))

    const button = screen.getByRole('button', { name: 'Continue' })
    expect(button.className).toContain('button--primary')
    expect(button.className).toContain('button--lg')
    expect(button.className).toContain('button--full')
    expect(button.className).not.toContain('button--rounded')
  })

  it('uses one class generator for button-like links', () => {
    const className = buttonClasses({ variant: 'ghost', size: 'xs', shape: 'pill' })

    expect(className).toContain('button--ghost')
    expect(className).toContain('button--xs')
    expect(className).toContain('button--pill')
  })
})
