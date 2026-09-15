import { useCallback, useEffect, useRef } from 'react'
import { shotUrl } from '../lib/asset'
import { CATEGORY_LABEL, type Screen } from '../lib/catalogue'

interface LightboxProps {
  items: readonly Screen[]
  index: number
  onClose: () => void
  onIndex: (next: number) => void
}

export function Lightbox({ items, index, onClose, onIndex }: LightboxProps) {
  const dialog = useRef<HTMLDivElement>(null)
  const current = items[index]

  const step = useCallback(
    (delta: number) => {
      const next = index + delta
      if (next < 0 || next >= items.length) return
      onIndex(next)
    },
    [index, items.length, onIndex],
  )

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        onClose()
      } else if (e.key === 'ArrowRight') {
        e.preventDefault()
        step(1)
      } else if (e.key === 'ArrowLeft') {
        e.preventDefault()
        step(-1)
      } else if (e.key === 'Home') {
        e.preventDefault()
        onIndex(0)
      } else if (e.key === 'End') {
        e.preventDefault()
        onIndex(items.length - 1)
      }
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [items.length, onClose, onIndex, step])

  useEffect(() => {
    const previous = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    dialog.current?.focus()
    return () => {
      document.body.style.overflow = previous
    }
  }, [])

  if (!current) return null

  return (
    <div
      className="lightbox"
      role="dialog"
      aria-modal="true"
      aria-label={`${current.title} — screenshot ${index + 1} of ${items.length}`}
      tabIndex={-1}
      ref={dialog}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="lightbox__top">
        <span className="lightbox__counter">
          {index + 1} / {items.length} · {CATEGORY_LABEL[current.category]}
        </span>
        <button type="button" className="lightbox__close" onClick={onClose}>
          Close · Esc
        </button>
      </div>

      <div className="lightbox__stage">
        <button
          type="button"
          className="lightbox__nav"
          onClick={() => step(-1)}
          disabled={index === 0}
          aria-label="Previous screenshot"
        >
          ←
        </button>
        <div className="lightbox__img">
          <img
            src={shotUrl(current.file)}
            alt={current.caption || current.title}
            width={528}
            height={792}
            decoding="async"
          />
        </div>
        <button
          type="button"
          className="lightbox__nav"
          onClick={() => step(1)}
          disabled={index === items.length - 1}
          aria-label="Next screenshot"
        >
          →
        </button>
      </div>

      <div className="lightbox__meta">
        <h2>{current.title}</h2>
        <p>{current.caption}</p>
        <p className="lightbox__hint">Arrow keys to move · Esc to close</p>
      </div>
    </div>
  )
}
