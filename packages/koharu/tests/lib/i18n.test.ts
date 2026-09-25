import { readdirSync, readFileSync } from 'node:fs'
import path from 'node:path'

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const values = new Map<string, string>()
const storage: Storage = {
  get length() {
    return values.size
  },
  clear: () => values.clear(),
  getItem: (key) => values.get(key) ?? null,
  key: (index) => [...values.keys()][index] ?? null,
  removeItem: (key) => values.delete(key),
  setItem: (key, value) => values.set(key, value),
}

describe('interface language persistence', () => {
  beforeEach(() => {
    Object.defineProperty(window, 'localStorage', { configurable: true, value: storage })
  })

  afterEach(() => {
    window.localStorage.clear()
    vi.resetModules()
  })

  it('stores an explicitly selected language', async () => {
    vi.resetModules()
    const { default: i18n } = await import('@/lib/i18n')

    await i18n.changeLanguage('ja-JP')

    expect(window.localStorage.getItem('i18nextLng')).toBe('ja-JP')
  })

  it('restores the saved language when i18n initializes', async () => {
    window.localStorage.setItem('i18nextLng', 'ja-JP')
    vi.resetModules()

    const { default: i18n } = await import('@/lib/i18n')

    expect(i18n.resolvedLanguage).toBe('ja-JP')
    expect(window.localStorage.getItem('i18nextLng')).toBe('ja-JP')
  })
})

describe('locale files', () => {
  const root = path.resolve(import.meta.dirname, '../../public/locales')

  function keysOf(locale: string): string[] {
    const document = JSON.parse(readFileSync(path.join(root, locale, 'translation.json'), 'utf8'))
    const walk = (value: unknown, prefix: string): string[] =>
      value !== null && typeof value === 'object'
        ? Object.entries(value as Record<string, unknown>).flatMap(([key, child]) =>
            walk(child, prefix ? `${prefix}.${key}` : key),
          )
        : [prefix]
    return walk(document, '').sort()
  }

  it('keeps every locale aligned on the English key set', () => {
    const english = keysOf('en-US')
    expect(english.length).toBeGreaterThan(400)

    for (const locale of readdirSync(root)) {
      if (locale === 'en-US') continue
      expect(keysOf(locale), `${locale} does not match en-US`).toEqual(english)
    }
  })

  it('ships the languages the settings screen can select', async () => {
    const { supportedLanguages } = await import('@/lib/i18n')
    for (const language of supportedLanguages) {
      expect(keysOf(language)).toContain(`languages.${language}`)
    }
    expect(supportedLanguages).toContain('fr-FR')
  })
})
