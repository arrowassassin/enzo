import { useEffect, useState } from 'react'

/**
 * Returns the id of the section currently nearest the top of the viewport.
 * Plain scroll maths rather than an IntersectionObserver, because sections here
 * are taller than the viewport and "is intersecting" says nothing useful then.
 */
export function useScrollSpy(ids: readonly string[], offset = 120): string {
  const [active, setActive] = useState<string>(ids[0] ?? '')

  useEffect(() => {
    let frame = 0

    const measure = () => {
      frame = 0
      let current = ids[0] ?? ''
      for (const id of ids) {
        const el = document.getElementById(id)
        if (!el) continue
        if (el.getBoundingClientRect().top - offset <= 0) current = id
      }
      // At the very bottom the last section may never reach the offset line.
      if (window.innerHeight + window.scrollY >= document.body.scrollHeight - 2) {
        current = ids[ids.length - 1] ?? current
      }
      setActive(current)
    }

    const onScroll = () => {
      if (frame) return
      frame = window.requestAnimationFrame(measure)
    }

    measure()
    window.addEventListener('scroll', onScroll, { passive: true })
    window.addEventListener('resize', onScroll)
    return () => {
      window.cancelAnimationFrame(frame)
      window.removeEventListener('scroll', onScroll)
      window.removeEventListener('resize', onScroll)
    }
  }, [ids, offset])

  return active
}
