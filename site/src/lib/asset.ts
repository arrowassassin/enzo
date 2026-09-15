/**
 * The site is served from a sub-path (https://arrowassassin.github.io/quire/),
 * so anything that lives in `public/` has to be resolved against the Vite base
 * rather than written as a root-absolute URL.
 */
export function asset(path: string): string {
  const base = import.meta.env.BASE_URL
  return `${base}${path.replace(/^\/+/, '')}`
}

/** Absolute URL for a shot in `public/shots`. */
export function shotUrl(file: string): string {
  return asset(`shots/${file}.png`)
}

/** The canonical origin the site is published under, for og:/canonical tags. */
export const SITE_ORIGIN = 'https://arrowassassin.github.io'
export const SITE_URL = `${SITE_ORIGIN}/quire/`
