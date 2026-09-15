import { Link } from 'react-router-dom'
import { FAQ, REPO } from '../data/site'
import { useSeo } from '../lib/useSeo'

export function FaqPage() {
  useSeo({
    title: 'FAQ',
    description:
      'Common questions about Quire: who makes it, how to get back to the stock firmware, which version is current, what it does with your data, and what happens if an update fails.',
    path: '/faq',
  })

  return (
    <>
      <header className="page-head">
        <div className="wrap">
          <span className="eyebrow">FAQ</span>
          <h1>Short answers.</h1>
          <p>
            If something is not answered here, the{' '}
            <a href={REPO} target="_blank" rel="noreferrer">
              repository
            </a>{' '}
            has the code and the issue tracker.
          </p>
        </div>
      </header>

      <section className="section">
        <div className="wrap">
          <div className="faq">
            {FAQ.map((item, i) => (
              <details key={item.q} open={i === 0}>
                <summary>{item.q}</summary>
                <div className="faq__body">
                  <p>{item.a}</p>
                </div>
              </details>
            ))}
          </div>

          <p style={{ marginTop: 'var(--s-8)' }}>
            <Link className="link-arrow" to="/guide">
              The guide answers the longer ones
            </Link>
          </p>
        </div>
      </section>
    </>
  )
}
