import { useCallback, useMemo, useState } from 'react'
import { Lightbox } from '../components/Lightbox'
import { SearchIcon } from '../components/Icons'
import {
  CATEGORY_LABEL,
  activeCategories,
  catalogue,
  screenCount,
  type ScreenCategory,
} from '../lib/catalogue'
import { shotUrl } from '../lib/asset'
import { useSeo } from '../lib/useSeo'

type Filter = ScreenCategory | 'all'

export function Screens() {
  useSeo({
    title: 'Screens',
    description: `Every screen in Quire's tour — ${screenCount} real 528 × 792 screenshots rendered by the simulator from the firmware's own code, filterable and searchable.`,
    path: '/screens',
  })

  const [filter, setFilter] = useState<Filter>('all')
  const [query, setQuery] = useState('')
  const [open, setOpen] = useState<number | null>(null)

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase()
    return catalogue.filter((s) => {
      if (filter !== 'all' && s.category !== filter) return false
      if (!q) return true
      return (
        s.title.toLowerCase().includes(q) ||
        s.caption.toLowerCase().includes(q) ||
        s.file.toLowerCase().includes(q)
      )
    })
  }, [filter, query])

  const close = useCallback(() => setOpen(null), [])

  return (
    <>
      <header className="page-head">
        <div className="wrap">
          <span className="eyebrow">Screens</span>
          <h1>The whole tour.</h1>
          <p>
            {screenCount} screenshots at the panel’s real 528 × 792, rendered by the headless
            simulator from the same code the device runs. These are the UI, not mockups — the
            snapshot suite fails if any of them changes by a pixel.
          </p>
        </div>
      </header>

      <div className="gallery__controls">
        <div className="wrap gallery__bar">
          <div className="search">
            <SearchIcon />
            <label className="visually-hidden" htmlFor="screen-search">
              Search the screens by title or caption
            </label>
            <input
              id="screen-search"
              type="search"
              placeholder="Search titles and captions…"
              value={query}
              onChange={(e) => {
                setQuery(e.target.value)
                setOpen(null)
              }}
              autoComplete="off"
            />
          </div>

          <ul className="chips">
            <li>
              <button
                type="button"
                className="chip"
                aria-pressed={filter === 'all'}
                onClick={() => {
                  setFilter('all')
                  setOpen(null)
                }}
              >
                All
              </button>
            </li>
            {activeCategories.map((c) => (
              <li key={c}>
                <button
                  type="button"
                  className="chip"
                  aria-pressed={filter === c}
                  onClick={() => {
                    setFilter(c)
                    setOpen(null)
                  }}
                >
                  {CATEGORY_LABEL[c]}
                </button>
              </li>
            ))}
          </ul>

          <span className="gallery__count" role="status">
            {visible.length} of {screenCount}
          </span>
        </div>
      </div>

      <section className="section section--tight">
        <div className="wrap">
          {visible.length === 0 ? (
            <p className="gallery__empty">
              Nothing matches “{query}”. Try a different word, or clear the filter.
            </p>
          ) : (
            <ul className="gallery__grid">
              {visible.map((screen, i) => (
                <li key={screen.file}>
                  <button
                    type="button"
                    className="tile"
                    onClick={() => setOpen(i)}
                    aria-label={`Open ${screen.title} full size`}
                  >
                    <div className="tile__frame">
                      <img
                        src={shotUrl(screen.file)}
                        alt={screen.caption || screen.title}
                        width={528}
                        height={792}
                        loading="lazy"
                        decoding="async"
                      />
                    </div>
                    <span className="tile__title">{screen.title}</span>
                    <span className="tile__cat">{CATEGORY_LABEL[screen.category]}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </section>

      {open !== null ? (
        <Lightbox items={visible} index={open} onClose={close} onIndex={setOpen} />
      ) : null}
    </>
  )
}
