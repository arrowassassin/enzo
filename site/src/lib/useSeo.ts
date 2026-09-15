import { useEffect } from 'react'
import { SITE_URL } from './asset'

interface Seo {
  title: string
  description: string
  path: string
}

function setMeta(selector: string, attr: 'name' | 'property', key: string, value: string) {
  let tag = document.head.querySelector<HTMLMetaElement>(selector)
  if (!tag) {
    tag = document.createElement('meta')
    tag.setAttribute(attr, key)
    document.head.appendChild(tag)
  }
  tag.setAttribute('content', value)
}

/** Per-route title, description, canonical and the og:/twitter: URL + title. */
export function useSeo({ title, description, path }: Seo): void {
  useEffect(() => {
    const full = path === '/' ? 'Quire — e-reader firmware for the Xteink X3' : `${title} — Quire`
    document.title = full

    setMeta('meta[name="description"]', 'name', 'description', description)
    setMeta('meta[property="og:title"]', 'property', 'og:title', full)
    setMeta('meta[property="og:description"]', 'property', 'og:description', description)
    setMeta('meta[name="twitter:title"]', 'name', 'twitter:title', full)
    setMeta('meta[name="twitter:description"]', 'name', 'twitter:description', description)

    const url = new URL(path.replace(/^\//, ''), SITE_URL).toString()
    setMeta('meta[property="og:url"]', 'property', 'og:url', url)

    let canonical = document.head.querySelector<HTMLLinkElement>('link[rel="canonical"]')
    if (!canonical) {
      canonical = document.createElement('link')
      canonical.rel = 'canonical'
      document.head.appendChild(canonical)
    }
    canonical.href = url
  }, [title, description, path])
}
