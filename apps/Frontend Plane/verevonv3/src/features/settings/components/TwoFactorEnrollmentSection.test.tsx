// @vitest-environment jsdom

import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it } from 'vitest'
import { TwoFactorEnrollmentSection } from './TwoFactorEnrollmentSection'

afterEach(() => cleanup())

describe('TwoFactorEnrollmentSection', () => {
  it('masks the step-up account password and marks it for password managers', () => {
    render(() => <TwoFactorEnrollmentSection />)

    const password = screen.getByLabelText('Kontopassord') as HTMLInputElement
    expect(password.type).toBe('password')
    expect(password.autocomplete).toBe('current-password')
  })
})
