import { useEffect } from 'react'

/**
 * One IntersectionObserver for every `.reveal` element on the page. Elements are
 * marked shown and then forgotten, so nothing animates twice and nothing is
 * observed for longer than it needs to be. Reduced-motion users get everything
 * shown immediately (the CSS also neutralises the transform).
 */
export function useReveal(deps: ReadonlyArray<unknown> = []): void {
  useEffect(() => {
    const nodes = Array.from(document.querySelectorAll<HTMLElement>('.reveal'))
    if (nodes.length === 0) return

    const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    if (reduce || typeof IntersectionObserver === 'undefined') {
      for (const node of nodes) node.dataset.shown = 'true'
      return
    }

    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue
          const el = entry.target as HTMLElement
          el.dataset.shown = 'true'
          observer.unobserve(el)
        }
      },
      { rootMargin: '0px 0px -8% 0px', threshold: 0.08 },
    )

    for (const node of nodes) {
      if (node.dataset.shown === 'true') continue
      observer.observe(node)
    }

    return () => observer.disconnect()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps)
}
