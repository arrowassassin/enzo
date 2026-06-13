# Spike 0 — Compositor & latency validation

Status: planned · Owner: TBD · Timebox: 2–4 weeks · Blocks: v0.1 architecture lock

The purpose of this spike is to make two **irreversible** decisions with data instead
of opinion (see `design/design-document.md` §4.1):

1. Is **wgpu** fast enough for our keystroke-to-glyph latency budget, or do we need a
   thinner native layer (`blade` / direct Metal·DX12·Vulkan)?
2. Do we **build our own compositor**, or adopt **GPUI**?

## What it builds
A throwaway binary that:
- Opens a `winit` window and a `wgpu` surface.
- Renders a scrolling monospace terminal grid (~200×50 cells) with a glyph atlas via
  `cosmic-text` + `swash`.
- Accepts keystrokes and echoes them at the cursor.
- Streams a large file (`cat`-style) to stress throughput.
- A parallel branch renders the same grid using **GPUI** for comparison.

## What it measures (pass/fail thresholds)

| Metric | Target | Hard fail |
|---|---|---|
| Keystroke → glyph on screen (p50) | ≤ 8 ms (1 frame @ 120 Hz) | > 16 ms |
| Keystroke → glyph (p99) | ≤ 16 ms | > 33 ms |
| Throughput: stream 1 GB | no dropped input, ≥ 1 GB/s parse | input stalls |
| Steady-state idle CPU | < 1% | > 5% |
| Cold start to interactive | < 100 ms | > 300 ms |
| Cross-platform | identical on macOS, Windows, Linux | any platform broken |

Latency is measured with a high-speed capture (240 fps) or a photodiode rig, not
wall-clock in-process timing, so it reflects real perceived latency.

## Decision matrix
- **wgpu meets p50 ≤ 8 ms** → adopt wgpu (keeps the future WebGPU client path).
- **wgpu misses but blade/native meets it** → drop to a thin native layer behind the
  renderer trait.
- **GPUI meets thresholds AND its panel/text model fits our dock + CRT-effects needs**
  → consider keeping it for v0.1, revisit own-compositor later.
- **Otherwise** → build own compositor (the documented default).

## Out of scope
No real PTY, no ATP, no themes beyond one. This spike is deliberately disposable; none
of its code ships. The output is a one-page memo recording the numbers and the
decision, appended here.

## Result
_(to be filled in when the spike completes)_
