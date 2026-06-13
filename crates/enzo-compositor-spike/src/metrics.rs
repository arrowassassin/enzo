//! Collects keystroke→glyph latency samples and prints a report on exit.

use std::time::Duration;

pub struct LatencyTracker {
    samples: Vec<Duration>,
}

impl LatencyTracker {
    pub fn new() -> Self {
        Self {
            samples: Vec::with_capacity(4096),
        }
    }

    pub fn record_keystroke_latency(&mut self, d: Duration) {
        self.samples.push(d);
    }

    pub fn report(&mut self) {
        if self.samples.is_empty() {
            println!("[spike] no keystrokes recorded");
            return;
        }
        self.samples.sort_unstable();
        let n = self.samples.len();
        let p50 = self.samples[n / 2];
        let p95 = self.samples[(n * 95) / 100];
        let p99 = self.samples[(n * 99) / 100];
        let max = self.samples[n - 1];

        println!("\n=== Keystroke→glyph latency ({n} samples) ===");
        println!("  p50  {:>8.2} ms  (target ≤8 ms, fail >16 ms)", ms(p50));
        println!("  p95  {:>8.2} ms", ms(p95));
        println!("  p99  {:>8.2} ms  (target ≤16 ms, fail >33 ms)", ms(p99));
        println!("  max  {:>8.2} ms", ms(max));

        let verdict_p50 = if ms(p50) <= 8.0 {
            "PASS"
        } else if ms(p50) <= 16.0 {
            "WARN"
        } else {
            "FAIL"
        };
        let verdict_p99 = if ms(p99) <= 16.0 {
            "PASS"
        } else if ms(p99) <= 33.0 {
            "WARN"
        } else {
            "FAIL"
        };
        println!("\n  p50 verdict: {verdict_p50}");
        println!("  p99 verdict: {verdict_p99}");
        println!();
        println!("NOTE: these are in-process timestamps (request_redraw → wgpu submit).");
        println!("For true perceived latency add ~1–2 frames of display pipeline lag.");
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}
