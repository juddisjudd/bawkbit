# bawkbit web UI

The browser side of bawkbit. It talks to the HTTP API in `crates/web` and is built into the bawkbit and bawkbitd binaries.

It is Svelte 5 with TypeScript, built by Vite. You need [bun](https://bun.sh) 1.4 or later.

## Build

```sh
bun install
bun run build    # writes web/dist
```

Then build bawkbit. `crates/web/build.rs` puts everything in `web/dist` inside the binary, so build the web UI first. A binary built without `web/dist` serves a page that says the web UI is not built.

## Work on it

1. Start a bawkbit daemon with its web UI on `127.0.0.1:8229`. Set it in the daemon's `settings.json`, under `web`.
2. Run `bun run dev` here. It serves the UI on `http://localhost:5173` and sends `/api` to the daemon.
3. Log in with the password the daemon printed when it started.

To point the dev server at another daemon, set `BAWKBIT_API`, for example `BAWKBIT_API=http://192.168.1.20:8228 bun run dev`.

## Check

```sh
bun run check    # svelte-check, fails on warnings
```

## Where things are

| File | What it holds |
| --- | --- |
| `src/lib/api.ts` | the API types, mirrored from `crates/core/src/api.rs` and `settings.rs`, and the fetch calls |
| `src/lib/state.svelte.ts` | app state: the live snapshot, filters, sort, selection, dialogs |
| `src/lib/components/` | one component per part of the screen |
| `src/app.css` | colors, type and shared controls, the same tokens as bawkterm |

When a Rust API type changes, change `api.ts` to match.
