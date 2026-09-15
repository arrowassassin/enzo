import { useEffect } from 'react'
import { useLocation } from 'react-router-dom'

/** A route change should start at the top; an in-page #hash should not. */
export function ScrollToTop() {
  const { pathname, hash } = useLocation()

  useEffect(() => {
    if (hash) return
    window.scrollTo({ top: 0, behavior: 'auto' })
  }, [pathname, hash])

  return null
}
