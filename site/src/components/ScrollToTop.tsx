import { useEffect } from 'react'
import { useLocation } from 'react-router-dom'

/** A route change starts at the top; a route with a #hash goes to that element. */
export function ScrollToTop() {
  const { pathname, hash } = useLocation()

  useEffect(() => {
    if (!hash) {
      window.scrollTo({ top: 0, behavior: 'auto' })
      return
    }
    // React Router does not jump to a #hash by itself, and the target may need a
    // frame or two: a route that has only just mounted, or a folded-away block
    // that opens itself when it sees the hash.
    const id = decodeURIComponent(hash.slice(1))
    const jump = () => document.getElementById(id)?.scrollIntoView({ block: 'start' })
    jump()
    const again = window.setTimeout(jump, 80)
    return () => window.clearTimeout(again)
  }, [pathname, hash])

  return null
}
