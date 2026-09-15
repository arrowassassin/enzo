/**
 * Reading the whole flash, the way esptool does it.
 *
 * `esptool-js` ships a `readFlash`, but it accumulates with
 * `resp = appendArray(resp, packet)` — a fresh allocation and a full copy of
 * everything received so far, for every packet that arrives. Over 16 MB that is
 * quadratic: on the order of a hundred gigabytes of copying. The read slows to a
 * crawl within the first megabyte, the stub times out waiting for its
 * acknowledgement, and the connection drops. It also asks the stub for 1024
 * blocks in flight, four megabytes of unacknowledged data, and never reads the
 * MD5 the stub sends at the end.
 *
 * This reads into one preallocated buffer, keeps the in-flight window at the 64
 * blocks esptool.py uses, and checks the digest of every chunk. Everything it
 * touches on the loader is public in esptool-js's own type definitions.
 */

import { md5Hex } from './md5'

/** Bytes the stub sends per block. */
const BLOCK = 0x1000
/** Blocks the stub may have unacknowledged. esptool.py uses 64; 256 KB in flight. */
const IN_FLIGHT = 64
/** Bytes per read-flash command. Bounds each command's time and digest. */
const CHUNK = 1024 * 1024
/** The stub signs off each command with an MD5. */
const DIGEST_BYTES = 16

/** The slice of esptool-js's ESPLoader this needs, all of it publicly typed. */
export interface FlashReader {
  ESP_READ_FLASH: number
  FLASH_READ_TIMEOUT: number
  transport: { read(timeout: number): Promise<Uint8Array>; write(data: Uint8Array): Promise<void> }
  _intToByteArray(i: number): Uint8Array
  _appendArray(a: Uint8Array, b: Uint8Array): Uint8Array
  checkCommand(
    opDescription?: string,
    op?: number | null,
    data?: Uint8Array,
    chk?: number,
    responseDataLength?: number,
    timeout?: number,
  ): Promise<number | Uint8Array>
}

export class FlashReadError extends Error {}

function hex(bytes: Uint8Array): string {
  return [...bytes].map((b) => b.toString(16).padStart(2, '0')).join('')
}

/**
 * Read `size` bytes from `addr` into one preallocated buffer.
 *
 * `onProgress` is called with the running total, at most about ten times a
 * second, so repainting never competes with draining the serial port.
 */
export async function readFlashInto(
  loader: FlashReader,
  addr: number,
  size: number,
  onProgress?: (done: number, total: number) => void,
): Promise<Uint8Array> {
  const out = new Uint8Array(size)
  let done = 0
  let lastTick = 0

  const tick = (force: boolean): void => {
    if (!onProgress) return
    const now = Date.now()
    if (force || now - lastTick >= 100) {
      lastTick = now
      onProgress(done, size)
    }
  }

  while (done < size) {
    const len = Math.min(CHUNK, size - done)

    let pkt = loader._appendArray(loader._intToByteArray(addr + done), loader._intToByteArray(len))
    pkt = loader._appendArray(pkt, loader._intToByteArray(BLOCK))
    pkt = loader._appendArray(pkt, loader._intToByteArray(IN_FLIGHT))

    const res = await loader.checkCommand('read flash', loader.ESP_READ_FLASH, pkt)
    if (res !== 0) {
      throw new FlashReadError(`The reader refused the read at ${addr + done} (code ${String(res)}).`)
    }

    let got = 0
    while (got < len) {
      const packet = await loader.transport.read(loader.FLASH_READ_TIMEOUT)
      if (!(packet instanceof Uint8Array) || packet.length === 0) {
        throw new FlashReadError(
          `The reader stopped sending after ${done + got} of ${size} bytes. The cable or the adapter is the usual cause.`,
        )
      }
      if (got + packet.length > len) {
        throw new FlashReadError(
          `The reader sent more than it was asked for at ${done + got} bytes, so the data cannot be trusted.`,
        )
      }
      out.set(packet, done + got)
      got += packet.length
      // The stub waits for a running total before it sends more.
      await loader.transport.write(loader._intToByteArray(got))
      tick(false)
    }

    const digest = await loader.transport.read(loader.FLASH_READ_TIMEOUT)
    if (!(digest instanceof Uint8Array) || digest.length !== DIGEST_BYTES) {
      throw new FlashReadError(
        `The reader did not sign off the block at ${done} bytes, so the data cannot be checked.`,
      )
    }
    const want = hex(digest)
    const mine = md5Hex(out.subarray(done, done + len))
    if (mine !== want) {
      throw new FlashReadError(
        `The block at ${done} bytes arrived corrupted: the reader signed it ${want} and it hashes to ${mine}. Nothing has been saved. Try again, and prefer a direct adapter to a multi-port dongle.`,
      )
    }

    done += len
    tick(true)
  }

  return out
}
