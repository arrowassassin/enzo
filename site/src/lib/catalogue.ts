import { screens, type Screen, type ScreenCategory } from '../data/screens'
import { shotFiles } from '../data/shots'
import { fallbackCaptions } from '../data/captions'

export type { Screen, ScreenCategory }

export const CATEGORY_ORDER: readonly ScreenCategory[] = [
  'reading',
  'library',
  'books',
  'apps',
  'games',
  'analytics',
  'sleep',
  'settings',
  'system',
]

export const CATEGORY_LABEL: Record<ScreenCategory, string> = {
  reading: 'Reading',
  library: 'Library',
  books: 'Getting books',
  apps: 'Apps',
  games: 'Games',
  analytics: 'Analytics',
  sleep: 'Sleep',
  settings: 'Settings',
  system: 'System',
}

/** The tour files are numbered by area; the prefix is the only thing to read. */
function categoryFor(file: string): ScreenCategory {
  if (file.startsWith('layout-')) return 'reading'
  const prefix = file.slice(0, 2).toLowerCase()
  const n = Number.parseInt(prefix, 10)
  if (prefix === '2a' || (n >= 20 && n <= 29)) return 'reading'
  if (Number.isNaN(n)) return 'system'
  if (n <= 2) return 'system'
  if (n >= 10 && n <= 19) return 'library'
  if (n >= 30 && n <= 39) return 'books'
  if (n >= 40 && n <= 49) return 'sleep'
  if (n >= 50 && n <= 59) return 'settings'
  if (n >= 60 && n <= 69) return 'analytics'
  if (n >= 70 && n <= 79) return 'apps'
  if (n >= 80 && n <= 89) return 'games'
  return 'system'
}

/** `11-library--1-11-library` → `Library · 2`; `40-sleep-pack` → `Sleep pack`. */
function titleFor(file: string): string {
  const [head = file, rest] = file.split('--')
  const step = rest ? Number.parseInt(rest, 10) : Number.NaN
  const words = head
    .replace(/^[0-9a-fA-F]{2}[a-z]?-/, '')
    .replace(/[-_]+/g, ' ')
    .trim()
  const title = words.charAt(0).toUpperCase() + words.slice(1)
  return Number.isNaN(step) ? title : `${title} · ${step + 1}`
}

function derive(file: string): Screen {
  const category = categoryFor(file)
  const title = titleFor(file)
  return {
    file,
    title,
    caption: fallbackCaptions[file] ?? `${title} — ${CATEGORY_LABEL[category].toLowerCase()}.`,
    category,
  }
}

/**
 * The catalogue the whole site reads. `src/data/screens.ts` is written by hand
 * (one entry per PNG); until it is filled in the tour is derived from the file
 * names so the gallery, the alt text and the feature strips all still work.
 */
export const catalogue: readonly Screen[] =
  screens.length > 0 ? screens : shotFiles.map(derive)

const byFile = new Map(catalogue.map((s) => [s.file, s]))

/** Never throws: an unknown file still gets a title, caption and category. */
export function screenFor(file: string): Screen {
  return byFile.get(file) ?? derive(file)
}

export function screensFor(files: readonly string[]): Screen[] {
  return files.map(screenFor)
}

/** Categories that actually have screens behind them, in tour order. */
export const activeCategories: readonly ScreenCategory[] = CATEGORY_ORDER.filter((c) =>
  catalogue.some((s) => s.category === c),
)

export const screenCount = catalogue.length
