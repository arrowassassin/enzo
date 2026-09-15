// GitHub Pages serves static files only: there is no server-side rewrite for the
// SPA routes, so 404.html is a byte-for-byte copy of index.html and the router
// picks the path back up on load. .nojekyll stops Pages from running Jekyll over
// the build (which would drop files and folders beginning with an underscore).
import { copyFileSync, writeFileSync, existsSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const dist = join(dirname(fileURLToPath(import.meta.url)), '..', 'dist')
const index = join(dist, 'index.html')

if (!existsSync(index)) {
  console.error('postbuild: dist/index.html is missing — did vite build run?')
  process.exit(1)
}

copyFileSync(index, join(dist, '404.html'))
writeFileSync(join(dist, '.nojekyll'), '')
console.log('postbuild: wrote dist/404.html and dist/.nojekyll')
