# OpenVoice UI

The React frontend for the Tauri shell (`crates/ov-app`). Vite builds it to
`dist/`, which `tauri.conf.json` embeds into the binary as `frontendDist`.

## Running it

```sh
npm install
npm run dev        # http://localhost:5199, mock data, no Rust needed
npm run build      # type-check and emit dist/
npm run lint       # oxlint
npx tsc --noEmit   # type-check only
```

`npm run dev` works standalone. Anything that calls into Rust is guarded by a
`__TAURI_INTERNALS__` check in `src/engine/settings.ts` and returns `null`
outside the shell, so the UI can be developed in a browser at full speed.

To see it inside the real window, build the Tauri shell instead — its
`beforeDevCommand` in `crates/ov-app/tauri.conf.json` starts this server on port
5199 for you, so do not run `npm run dev` at the same time.

## Layout

| Path | What it is |
|---|---|
| `src/windows/` | One file per Tauri window: `Hub` (main), `Overlay` (the Flow Bar), `Sheet` (component gallery, `?window=sheet`) |
| `src/screens/` | Sections of the hub: Settings, Dictionary, Profiles, Advanced |
| `src/ui/` | The primitives everything else is built from |
| `src/engine/` | The bridge to Rust: event stream, settings, stats |
| `src/styles/` | Design tokens, then global styles. Tokens first; components never invent a colour |

## The one rule

**The UI is a projection of the event stream, never a source of truth.** It
renders whatever `ov-core` last said and computes nothing the engine could
compute. That is what keeps the engine testable without a window, and it is why
`src/engine/types.ts` is a hand-maintained mirror of `ov_core::event::Event`
rather than a place to add fields.

If you find yourself deriving state here, add it to the event instead.
