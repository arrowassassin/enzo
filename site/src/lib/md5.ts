/**
 * MD5, only because the chip speaks it. The flasher stub ends every read-flash
 * command with an MD5 of what it sent, and that digest is the one check that the
 * 16 MB which arrived is the 16 MB the reader holds. `crypto.subtle` does not
 * implement MD5 (deliberately: it is broken for signatures), and this use is not a
 * security one, so it lives here. RFC 1321.
 */

const S = [
  7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14,
  20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6,
  10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
]

/** K[i] = floor(2^32 * abs(sin(i + 1))), the table from the RFC. */
const K = new Uint32Array(64)
for (let i = 0; i < 64; i += 1) {
  K[i] = Math.floor(Math.abs(Math.sin(i + 1)) * 4294967296)
}

function rotl(x: number, c: number): number {
  return (x << c) | (x >>> (32 - c))
}

/**
 * The MD5 of `data`, lower-case hex.
 *
 * Padding is computed rather than materialised: a 16 MB copy just to append a
 * length would defeat the point of streaming the read in the first place.
 */
export function md5Hex(data: Uint8Array): string {
  const len = data.length
  const bitLenLo = (len << 3) >>> 0
  const bitLenHi = Math.floor(len / 536870912) >>> 0
  // The padded message: the data, 0x80, zeroes, then the 64-bit bit length.
  const total = (((len + 8) >> 6) + 1) << 6
  const words = new Uint32Array(16)

  let a0 = 0x67452301
  let b0 = 0xefcdab89
  let c0 = 0x98badcfe
  let d0 = 0x10325476

  const byteAt = (i: number): number => {
    if (i < len) return data[i] as number
    if (i === len) return 0x80
    if (i >= total - 8) {
      const k = i - (total - 8)
      const half = k < 4 ? bitLenLo : bitLenHi
      return (half >>> ((k % 4) * 8)) & 0xff
    }
    return 0
  }

  for (let off = 0; off < total; off += 64) {
    for (let w = 0; w < 16; w += 1) {
      const i = off + w * 4
      words[w] =
        (byteAt(i) | (byteAt(i + 1) << 8) | (byteAt(i + 2) << 16) | (byteAt(i + 3) << 24)) >>> 0
    }

    let a = a0
    let b = b0
    let c = c0
    let d = d0

    for (let i = 0; i < 64; i += 1) {
      let f: number
      let g: number
      if (i < 16) {
        f = (b & c) | (~b & d)
        g = i
      } else if (i < 32) {
        f = (d & b) | (~d & c)
        g = (5 * i + 1) % 16
      } else if (i < 48) {
        f = b ^ c ^ d
        g = (3 * i + 5) % 16
      } else {
        f = c ^ (b | ~d)
        g = (7 * i) % 16
      }
      const tmp = d
      d = c
      c = b
      const sum = (a + f + (K[i] as number) + (words[g] as number)) >>> 0
      b = (b + rotl(sum, S[i] as number)) >>> 0
      a = tmp
    }

    a0 = (a0 + a) >>> 0
    b0 = (b0 + b) >>> 0
    c0 = (c0 + c) >>> 0
    d0 = (d0 + d) >>> 0
  }

  let out = ''
  for (const word of [a0, b0, c0, d0]) {
    for (let i = 0; i < 4; i += 1) {
      out += (((word >>> (i * 8)) & 0xff) as number).toString(16).padStart(2, '0')
    }
  }
  return out
}
