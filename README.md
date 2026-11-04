# bawkbit

bawkbit is a BitTorrent client for Windows and Linux. It is a small native app, and it has a web UI so you can manage your torrents from a phone or another computer. It also runs without a window, as a service on a server or in Docker.

## Download

Get the newest version from the [releases page](https://github.com/juddisjudd/bawkbit/releases).

| System | Download | Install |
| --- | --- | --- |
| Windows 10 or 11, 64-bit | `bawkbit-…-windows-x64-setup.exe` | run it; no administrator needed |
| Windows, no installer | `bawkbit-…-windows-x64.zip` | unzip anywhere, start `bawkbit.exe` |
| Any Linux | `bawkbit-…-linux-x64.AppImage` | `chmod +x bawkbit-*.AppImage`, then run it |
| Debian, Ubuntu, Mint | `bawkbit_…_amd64.deb` | `sudo apt install ./bawkbit_*.deb` |
| Fedora | `bawkbit-….x86_64.rpm` | `sudo dnf install ./bawkbit-*.rpm` |
| A Linux server | `bawkbitd-…-linux-x64.tar.gz` | unpack, run `./bawkbitd` |
| Docker | `ghcr.io/juddisjudd/bawkbit` | see [Without a window](#without-a-window) |

The Linux downloads need a 64-bit system from 2022 or later (glibc 2.35), such as Ubuntu 22.04 or Debian 12. Each download has a `.sha256` file next to it to check it with.

**"Windows protected your PC"?** bawkbit is not signed with a paid certificate, so Windows may warn you the first time. Click **More info**, then **Run anyway**.

## Get started

### The desktop app

1. Start `bawkbit`.
2. Press **+**, paste a magnet link, or drop a `.torrent` file on the window.
3. Pick the files you want, then press **add**.

Downloads go to a `bawkbit` folder inside your Downloads folder. Change it under **settings → downloads**.

To open magnet links and `.torrent` files from your browser with bawkbit on Windows, go to **settings → downloads** and press **open them with bawkbit**.

### The web UI

1. Open **settings → web ui** in the desktop app.
2. Set a password and press **save**.
3. Turn on **web ui**. Open the address it shows.

To reach it from other devices, keep the address `0.0.0.0` and open `http://<this computer's IP>:8228`. With `127.0.0.1` only this computer can reach it.

### Search

bawkbit searches through [Prowlarr](https://prowlarr.com) or [Jackett](https://github.com/Jackett/Jackett), the indexer managers Sonarr and Radarr use. It ships no site scrapers of its own, so you pick the trackers, and Prowlarr or Jackett keeps them working when the sites change.

1. Open **settings → search** and press **add prowlarr** or **add jackett**.
2. Type its address, such as `http://tower:9696`, and its API key. Prowlarr shows the key under **settings → general**; Jackett shows it at the top right of its dashboard.
3. Press **test**. It says how many indexers it found.
4. Open **search**, type a name and press Enter. Click a result to add it.

Results come from every source you turn on at once. A torrent that several trackers list shows once. Type in the refine box to narrow the results: `-word` leaves a word out, and `>1GB` or `<500MB` limits the size.

### Updates

bawkbit asks GitHub for a new version once a day. When there is one, a dot shows on **settings**. Open **settings → about**, press **update**, then **restart now**. bawkbit checks the download against its SHA-256 checksum before it replaces anything, and your torrents carry on after the restart.

The Windows installer, the Windows zip and the AppImage update this way. Update the `.deb` and `.rpm` with your package manager, and Docker by pulling the new image. Turn the daily check off under **settings → about**.

### Without a window

`bawkbitd` is the engine and the web UI on their own. Start it, and it prints a password for the web UI on first start:

```
web UI login: admin / 9pp78qhhevuyef
```

Open `http://<server>:8228` and log in. Run `bawkbitd --new-password` to get a new password.

With Docker:

```sh
docker run -d --name bawkbit --restart unless-stopped \
  -p 8228:8228 -p 51413:51413 \
  -v ./config:/config -v ./downloads:/downloads \
  ghcr.io/juddisjudd/bawkbit:latest
docker logs bawkbit          # shows the web UI password
```

Settings live in `./config`, downloads in `./downloads`. Forward TCP port 51413 on your router so other peers can reach you. The `edge` tag follows every change on `main`. To build the image yourself instead, run `docker compose -f packaging/docker/docker-compose.yml up -d` from the repository root.

## Keys

| Press | To |
| --- | --- |
| `Ctrl+O` | add `.torrent` files |
| `Ctrl+V` | add the magnet link on the clipboard |
| `Space` | pause or resume the selected torrents |
| `Delete` | remove the selected torrents and keep their files |
| `Shift+Delete` | remove the selected torrents and delete their files |
| `Ctrl+A` | select every torrent in the list |
| `Ctrl+F` | filter the list by name |
| `Esc` | close the details |

Double-click a torrent to open its folder. Right-click it for everything else.

## What it does

- **Adds** magnet links, links to `.torrent` files and `.torrent` files. You see the file list first and pick what to download. Magnet links can also go straight in (**settings → downloads → add magnets without asking**).
- **Search** your Prowlarr or Jackett, with results from every tracker merged into one list.
- **Categories** with their own folder, and **tags**. Filter the list by status, category and tag at once.
- **Speed limits**, with alternative limits behind the turtle in the title bar and a schedule that switches to them.
- **A queue**: only so many downloads run at once, the rest wait their turn.
- **Seeding goals**: stop at a ratio or after a time.
- **A watch folder**: `.torrent` files put there are added on their own.
- **Details** for each torrent: a piece map, files with per-file progress and selection, peers and trackers.
- **DHT**, UPnP port forwarding and peer exchange through the engine.
- **One instance**: opening a magnet link while bawkbit runs hands it to the open window.
- **Updates itself** from the releases page, after checking each download's checksum.
- **Themes**: dark, light, follow your system, or one of 30 named themes.

bawkbit takes its ideas from two older clients. From [qBittorrent](https://github.com/qbittorrent/qBittorrent): categories with save paths, tags, the filter sidebar, alternative speed limits with a schedule, the queue and the built-in web UI. From [Deluge](https://github.com/deluge-torrent/deluge): the split between the engine and the windows that show it, so the same engine runs behind the desktop app, the web UI and the headless daemon.

## Where your things are

| What | Windows | Linux |
| --- | --- | --- |
| Downloads (change it under settings) | `Downloads\bawkbit` | `~/Downloads/bawkbit` |
| Engine settings | `%APPDATA%\bawkbit\settings.json` | `~/.config/bawkbit/settings.json` |
| Categories, tags and torrent history | `%APPDATA%\bawkbit\torrents.json` | `~/.config/bawkbit/torrents.json` |
| The torrents themselves, for resuming | `%APPDATA%\bawkbit\session` | `~/.config/bawkbit/session` |
| Desktop app look and sort order | `%APPDATA%\bawkbit\desktop.json` | `~/.config/bawkbit/desktop.json` |

Set `BAWKBIT_HOME` to a folder to keep a second copy's settings apart from your own.

## Build

You need Rust 1.92 or later and [bun](https://bun.sh). On Windows you also need the MSVC toolchain. On Linux you need a C compiler and the development files for fontconfig, Wayland, X11 and xkbcommon. On Debian and Ubuntu:

```sh
sudo apt install build-essential pkg-config libfontconfig-dev libwayland-dev libx11-xcb-dev libxkbcommon-x11-dev
```

Build the web UI first, because the Rust build puts it inside the program:

```sh
cd web && bun install && bun run build && cd ..
cargo run --release              # the desktop app
cargo run --release -p bawkbitd  # the engine and web UI without a window
cargo test --workspace           # the tests
```

The first build compiles GPUI and takes a few minutes. Without `web/dist`, the web UI shows a page that says it is not built.

To see what the desktop app does, set `LOG_LEVEL` and `LOG_FILE`. In PowerShell:

```powershell
$env:LOG_LEVEL = "DEBUG"; $env:LOG_FILE = "$env:TEMP\bawkbit.log"; cargo run
```

`bawkbitd` logs to the terminal. Set `RUST_LOG=debug` for more.

### How it fits together

| Part | What it is |
| --- | --- |
| [`crates/core`](crates/core) | The engine: torrents, settings, the queue, seeding goals, the schedule and the watch folder. Every client talks to it through the types in `api.rs`. |
| [`crates/web`](crates/web) | The HTTP API and the web UI server, with login, live updates and the built web UI inside. |
| [`crates/daemon`](crates/daemon) | `bawkbitd`, the engine and the web UI without a window. |
| [`src`](src) | The desktop app, drawn with GPUI. |
| [`web`](web) | The web UI, in Svelte. See [web/README.md](web/README.md). |

### Builds and releases

The [build workflow](.github/workflows/build.yml) runs on every push to `main` and every pull request. It checks the web UI, formats, lints and tests the Rust code on Windows and Linux, and packages everything. Download the packages from the run's page under **Artifacts**. Each push to `main` also publishes the `edge` Docker image.

To publish a version, run this from a clean `main`:

```powershell
.\scripts\release.ps1 -Version 0.2.0
```

It sets the version in `Cargo.toml` and `web/package.json`, commits, tags `v0.2.0` and pushes. The workflow then publishes the Windows installer and zip, the AppImage, the `.deb`, the `.rpm` and the tar.gz files on the releases page, each with its SHA-256 checksum. It also pushes the Docker image as `0.2.0`, `0.2` and `latest`. A tag that does not match the version in `Cargo.toml` fails the build.

The Windows installer comes from [`packaging/windows`](packaging/windows) and the Linux packages from [`packaging/linux/package.sh`](packaging/linux/package.sh).

### The HTTP API

The web UI uses this API, and so can your scripts. Log in with `POST /api/login` and `{"username", "password"}`; the answer sets a cookie. Send `content-type: application/json` with every write.

| Request | Does |
| --- | --- |
| `GET /api/snapshot` | every torrent, the speeds, categories and tags |
| `GET /api/events` | the same snapshot once a second, as server-sent events |
| `GET /api/torrents/{id}` | one torrent's files, peers, trackers and piece map |
| `POST /api/preview` | reads a magnet link or `.torrent` file without adding it |
| `POST /api/torrents` | adds a torrent |
| `POST /api/search` | searches every turned-on Prowlarr and Jackett |
| `POST /api/indexers/test` | checks a Prowlarr or Jackett address and key |
| `POST /api/actions` | pause, resume, remove, set files, categories, tags and the alternative limits |
| `GET` or `PUT /api/settings` | reads or changes the engine settings |

The request and answer shapes are in [`crates/core/src/api.rs`](crates/core/src/api.rs). A torrent's `id` is its info hash.

## Credits

- [librqbit](https://github.com/ikatson/rqbit) (Apache-2.0) is the BitTorrent engine.
- [GPUI Kit](https://github.com/longbridge/gpui-kit) (Apache-2.0) and Zed's GPUI draw the desktop app.
- [axum](https://github.com/tokio-rs/axum) (MIT) serves the web UI. [Svelte](https://svelte.dev) (MIT) builds it.
- [IBM Plex Mono](https://github.com/IBM/plex) (SIL Open Font License, `assets/fonts/OFL.txt`) is the typeface.
- [Lucide](https://lucide.dev) (ISC) icons.

## License

[AGPL-3.0](LICENSE).
