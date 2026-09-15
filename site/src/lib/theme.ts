import { useCallback, useEffect, useState } from 'react'

export type Theme = 'light' | 'dark' | 'system'

const KEY = 'quire-theme'

function read(): Theme {
  try {
    const v = localStorage.getItem(KEY)
    if (v === 'light' || v === 'dark' || v === 'system') return v
  } catch {
    /* private mode, blocked storage: fall through to the system preference */
  }
  return 'system'
}

function apply(theme: Theme): void {
  const root = document.documentElement
  if (theme === 'system') root.removeAttribute('data-theme')
  else root.setAttribute('data-theme', theme)
}

function resolved(theme: Theme): 'light' | 'dark' {
  if (theme !== 'system') return theme
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

export function useTheme(): {
  theme: Theme
  effective: 'light' | 'dark'
  toggle: () => void
} {
  const [theme, setTheme] = useState<Theme>(() =>
    typeof document === 'undefined' ? 'system' : read(),
  )
  const [systemDark, setSystemDark] = useState(
    () =>
      typeof window !== 'undefined' &&
      window.matchMedia('(prefers-color-scheme: dark)').matches,
  )

  useEffect(() => {
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const onChange = (e: MediaQueryListEvent) => setSystemDark(e.matches)
    mq.addEventListener('change', onChange)
    return () => mq.removeEventListener('change', onChange)
  }, [])

  useEffect(() => {
    apply(theme)
    try {
      if (theme === 'system') localStorage.removeItem(KEY)
      else localStorage.setItem(KEY, theme)
    } catch {
      /* nothing to persist to; the in-memory choice still holds for this visit */
    }
  }, [theme])

  const toggle = useCallback(() => {
    setTheme((current) => (resolved(current) === 'dark' ? 'light' : 'dark'))
  }, [])

  const effective: 'light' | 'dark' =
    theme === 'system' ? (systemDark ? 'dark' : 'light') : theme

  return { theme, effective, toggle }
}

/** Runs before React mounts so the first paint is already in the right palette. */
export function applyStoredTheme(): void {
  apply(read())
}
