# vendor

## librqbit

`librqbit/` is [librqbit](https://github.com/ikatson/rqbit) 9.1.0 (Apache-2.0, see `librqbit/LICENSE`) from crates.io, without its web UI, with the changes in [`librqbit.patch`](librqbit.patch). The root `Cargo.toml` uses it in place of the crates.io copy through `[patch.crates-io]`.

The changes:

- Each torrent has an upload switch that can change while it runs: `ManagedTorrent::set_upload_disabled`. With it on, the torrent never tells peers which pieces it has. Stock librqbit has this switch only for the whole session, and only at build time.
- Each torrent keeps its speed limits outside its running state, so they can change at any time and outlast a pause: `ManagedTorrent::ratelimits`.
- A rate limit always lets at least 128 KiB through at once. Before, a limit below one request (16 KiB/s) failed every request and dropped the peer.

### Upgrading librqbit

1. Copy the new version from `~/.cargo/registry/src/*/librqbit-<version>` over `librqbit/`. Leave out `webui/`, `.cargo-ok`, `.cargo_vcs_info.json`, `Cargo.lock` and `Cargo.toml.orig`.
2. From `librqbit/`, run `patch -p1 < ../librqbit.patch` and fix any part that does not apply.
3. Update librqbit's version in `crates/core/Cargo.toml`, then run `cargo test --workspace`.
4. Write the new patch: run `diff -ru` between the crates.io copy and `librqbit/src`, with `a/src/` and `b/src/` as the path prefixes.
