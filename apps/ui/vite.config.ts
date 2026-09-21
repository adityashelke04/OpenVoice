import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

/** The version the app reports, read at build time from the workspace manifest.
 *
 *  `Cargo.toml` is the source: a release bumps `[workspace.package].version`
 *  there and `crates/ov-app/tauri.conf.json` follows it. Baking the same number
 *  into the bundle keeps the Settings Version row from becoming a third place
 *  that has to be remembered, and from going quietly stale when it is not. */
function workspaceVersion() {
  const cargo = readFileSync(fileURLToPath(new URL('../../Cargo.toml', import.meta.url)), 'utf8')
  const block = cargo.slice(cargo.indexOf('[workspace.package]'))
  const found = /^version\s*=\s*"([^"]+)"/m.exec(block)
  if (!found) throw new Error('no [workspace.package] version in Cargo.toml')
  return found[1]
}

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  define: { __APP_VERSION__: JSON.stringify(workspaceVersion()) },
})
