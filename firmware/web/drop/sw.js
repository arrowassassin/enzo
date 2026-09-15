// Minimal service worker: makes the Drop page installable (and share-target capable on
// Android). Everything is fetched live from the reader; nothing is cached, so the page
// never goes stale after a firmware update.
self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', (e) => e.waitUntil(self.clients.claim()));
self.addEventListener('fetch', () => {});
