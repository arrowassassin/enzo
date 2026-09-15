import { Link } from 'react-router-dom'
import { useSeo } from '../lib/useSeo'

export function NotFound() {
  useSeo({
    title: 'Page not found',
    description: 'That page does not exist on the Quire site.',
    path: '/',
  })

  return (
    <section className="wrap notfound">
      <span className="eyebrow">404</span>
      <h1>That page is not in this quire.</h1>
      <p className="lead" style={{ marginInline: 'auto' }}>
        The link may be old, or the page may never have existed. Everything the site has is one
        step away.
      </p>
      <p style={{ marginTop: 'var(--s-6)' }}>
        <Link className="btn" to="/">
          Back to the front
        </Link>
      </p>
    </section>
  )
}
