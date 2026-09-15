/**
 * Helpers for the browser installer: formatting, hashing, validation and
 * handing a finished file to the browser. Nothing in here touches the serial
 * port, so it stays out of the lazily-loaded esptool-js chunk.
 */
import { FLASH_BYTES } from '../data/site'

const NUM = new Intl.NumberFormat('en-GB')

/** "16,777,216" — the exact byte count, the way the guide writes it. */
export function bytes(n: number): string {
  return NUM.format(n)
}

/** "16.0 MB" — a rough size for a progress line, never for verification. */
export function megabytes(n: number): string {
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}

/** A deliberately vague duration: "about 3 min", "about 40 s". */
export function roughly(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return ''
  if (seconds < 20) return 'a few seconds'
  if (seconds < 90) return `about ${Math.round(seconds / 10) * 10} s`
  return `about ${Math.round(seconds / 60)} min`
}

/** The file name the backup is saved under: xteink-x3-stock-2026-09-15.bin */
export function backupName(now: Date): string {
  const p = (n: number) => String(n).padStart(2, '0')
  return `xteink-x3-stock-${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}.bin`
}

/**
 * SHA-256 as lower-case hex. `crypto.subtle` only exists in a secure context;
 * so does the Web Serial API, so in practice both are there or neither is —
 * but a missing digest must not lose someone their backup, so this returns
 * null rather than throwing and the caller saves the .bin either way.
 */
export async function sha256Hex(data: Uint8Array): Promise<string | null> {
  if (typeof crypto === 'undefined' || !crypto.subtle) return null
  try {
    const copy = new Uint8Array(data)
    const digest = await crypto.subtle.digest('SHA-256', copy.buffer)
    return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('')
  } catch {
    return null
  }
}

/**
 * A Blob from a byte array. The copy is not ceremony: a Uint8Array may be a
 * view onto a shared or oversized buffer, and BlobPart will not take one.
 */
export function blobOf(data: Uint8Array, type: string): Blob {
  return new Blob([new Uint8Array(data)], { type })
}

/** Hands a blob to the browser as a download. Returns once the click is made. */
export function saveFile(name: string, blob: Blob): void {
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = name
  a.rel = 'noopener'
  a.style.display = 'none'
  document.body.appendChild(a)
  a.click()
  a.remove()
  // Revoking immediately can cancel the download in some browsers.
  window.setTimeout(() => URL.revokeObjectURL(url), 30_000)
}

/**
 * Checks a file is a whole-flash image for this device: exactly 16,777,216
 * bytes, starting with the ESP32 image magic byte. Returns a sentence to show
 * the reader, or null when the file is good.
 */
export function imageProblem(data: Uint8Array, name: string): string | null {
  if (data.length !== FLASH_BYTES) {
    return `${name} is ${bytes(data.length)} bytes. A whole-flash image for the X3 is exactly ${bytes(
      FLASH_BYTES,
    )} bytes, so this is not one — it may be a single-partition file such as quire-x3.bin, or a truncated download.`
  }
  if (data[0] !== 0xe9) {
    return `${name} is the right length but does not begin with 0xE9, the marker every ESP32 flash image starts with. It is not a flash image for this chip.`
  }
  return null
}

export interface Progress {
  done: number
  total: number
  /** Seconds remaining, once there is enough of a sample to guess from. */
  eta: number | null
}

/**
 * Wraps a state setter in a throttle and an estimate. Long reads and writes
 * call back thousands of times; React does not need to hear all of them.
 */
export function tracker(set: (p: Progress) => void): (done: number, total: number) => void {
  const started = performance.now()
  let last = 0
  return (done, total) => {
    const now = performance.now()
    if (done < total && now - last < 120) return
    last = now
    const elapsed = (now - started) / 1000
    const ratio = total > 0 ? done / total : 0
    const eta = ratio > 0.02 && ratio < 1 ? (elapsed / ratio) * (1 - ratio) : null
    set({ done, total, eta })
  }
}

export function percent(p: Progress): number {
  if (p.total <= 0) return 0
  return Math.min(100, Math.floor((p.done / p.total) * 100))
}
