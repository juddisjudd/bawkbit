# vendor

## librqbit

`librqbit/` is [librqbit](https://github.com/ikatson/rqbit) 9.1.0 (Apache-2.0, see `librqbit/LICENSE`) from crates.io, without its web UI, with the changes in [`librqbit.patch`](librqbit.patch). The root `Cargo.toml` uses it in place of the crates.io copy through `[patch.crates-io]`.

The changes:

- Each torrent has an upload switch that can change while it runs: `ManagedTorrent::set_upload_disabled`. With it on, the torrent never tells peers which pieces it has. Stock librqbit has this switch only for the whole session, and only at build time.
- Each torrent keeps its speed limits outside its running state, so they can change at any time and outlast a pause: `ManagedTorrent::ratelimits`.
- Torrents can share a second set of limits, so a group of them stays under one speed together: `ManagedTorrent::set_group_ratelimits`. bawkbit uses it for category limits.
- A rate limit always lets at least 128 KiB through at once. Before, a limit below one request (16 KiB/s) failed every request and dropped the peer.
- The session keeps what each tracker of each torrent last said: `Session::tracker_status`. bawkbit shows it and uses it to find dead trackers.

## librqbit-tracker-comms

`librqbit-tracker-comms/` is librqbit's tracker crate, also 9.1.0 and Apache-2.0, with the changes in [`librqbit-tracker-comms.patch`](librqbit-tracker-comms.patch). The root `Cargo.toml` patches it in the same way.

The changes:

- Each announce writes its result into a `TrackerStatus` that the caller passes to `TrackerComms::start`: working or failing, the error, the peers, seeds and leechers it gave, and how long it has failed.
- An HTTP tracker that does not answer in 30 seconds counts as failed and is asked again later. Before, a tracker that never answered held its slot for good.
- HTTP errors leave out the announce address, so messages stay short and the peer id stays out of them.

### Upgrading librqbit

1. Copy the new version from `~/.cargo/registry/src/*/librqbit-<version>` over `librqbit/`. Leave out `webui/`, `.cargo-ok`, `.cargo_vcs_info.json`, `Cargo.lock` and `Cargo.toml.orig`.
2. Do the same for `librqbit-tracker-comms-<version>` over `librqbit-tracker-comms/`.
3. In each folder, run `patch -p1 < ../<folder>.patch` and fix any part that does not apply.
4. Update librqbit's version in `crates/core/Cargo.toml`, then run `cargo test --workspace`.
5. Write the new patches: run `diff -ru` between each crates.io copy and the folder's `src`, with `a/src/` and `b/src/` as the path prefixes.
