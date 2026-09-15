/**
 * Looking up, and downloading, the factory image from a published GitHub
 * release.
 *
 * No `v*` tag has been cut yet, so `none` is the answer people actually get
 * today. That is not an error to hide: the page says so plainly and points at
 * the CI artifact instead. The page asks first and only offers the button when
 * there is really something behind it — no dead or invented download.
 */
import { FACTORY_IMAGE, RELEASES_API } from '../data/site'

export type ReleaseProbe =
  | { state: 'available'; tag: string; asset: string; url: string; size: number }
  | { state: 'none' }
  | { state: 'no-asset'; tag: string }
  | { state: 'unknown'; reason: string }

interface Asset {
  name: string
  url: string
  size: number
}

function readRelease(body: unknown): { tag: string; assets: Asset[] } {
  if (typeof body !== 'object' || body === null) return { tag: '', assets: [] }
  const record = body as Record<string, unknown>
  const tag = typeof record['tag_name'] === 'string' ? record['tag_name'] : ''
  const raw = Array.isArray(record['assets']) ? record['assets'] : []
  const assets: Asset[] = []
  for (const entry of raw) {
    if (typeof entry !== 'object' || entry === null) continue
    const a = entry as Record<string, unknown>
    const name = a['name']
    const url = a['browser_download_url']
    const size = a['size']
    if (typeof name === 'string' && typeof url === 'string') {
      assets.push({ name, url, size: typeof size === 'number' ? size : 0 })
    }
  }
  return { tag, assets }
}

/** Asks GitHub whether a release carrying the factory image exists. */
export async function probeLatestRelease(signal?: AbortSignal): Promise<ReleaseProbe> {
  let res: Response
  try {
    res = await fetch(RELEASES_API, {
      headers: { Accept: 'application/vnd.github+json' },
      ...(signal ? { signal } : {}),
    })
  } catch {
    return { state: 'unknown', reason: 'GitHub could not be reached from this browser.' }
  }

  if (res.status === 404) return { state: 'none' }
  if (!res.ok) {
    return { state: 'unknown', reason: `GitHub answered ${res.status} when asked for the latest release.` }
  }

  let body: unknown
  try {
    body = await res.json()
  } catch {
    return { state: 'unknown', reason: 'GitHub’s answer could not be read as a release.' }
  }

  const { tag, assets } = readRelease(body)
  const asset = assets.find((a) => a.name === FACTORY_IMAGE)
  if (!asset) return { state: 'no-asset', tag }
  return { state: 'available', tag, asset: asset.name, url: asset.url, size: asset.size }
}

/** Streams a release asset, reporting bytes received against its known size. */
export async function downloadAsset(
  probe: Extract<ReleaseProbe, { state: 'available' }>,
  onProgress: (done: number, total: number) => void,
  signal?: AbortSignal,
): Promise<Uint8Array> {
  const res = await fetch(probe.url, signal ? { signal } : {})
  if (!res.ok) {
    throw new Error(
      `Downloading ${probe.asset} failed with ${res.status}. Try again, or download the file yourself and use the file picker.`,
    )
  }

  const declared = Number(res.headers.get('content-length') ?? '')
  const total = Number.isFinite(declared) && declared > 0 ? declared : probe.size
  const body = res.body

  if (!body) {
    const whole = new Uint8Array(await res.arrayBuffer())
    onProgress(whole.length, whole.length)
    return whole
  }

  const reader = body.getReader()
  const chunks: Uint8Array[] = []
  let done = 0
  for (;;) {
    const next = await reader.read()
    if (next.done) break
    chunks.push(next.value)
    done += next.value.length
    onProgress(done, total > 0 ? total : done)
  }

  const data = new Uint8Array(done)
  let at = 0
  for (const chunk of chunks) {
    data.set(chunk, at)
    at += chunk.length
  }
  onProgress(done, done)
  return data
}
