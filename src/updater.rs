use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bawkbit_core::VERSION;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

/// Given to the copy an update starts, so it waits for this one to quit instead of handing its arguments over.
pub const AFTER_UPDATE: &str = "--after-update";
const LATEST_API: &str = "https://api.github.com/repos/juddisjudd/bawkbit/releases/latest";
#[cfg(windows)]
const ASSET_SUFFIX: &str = "-windows-x64.zip";
#[cfg(not(windows))]
const ASSET_SUFFIX: &str = "-linux-x64.AppImage";
/// The installer's entry under installed apps, named after the AppId in `packaging/windows/bawkbit.iss`.
#[cfg(windows)]
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{8C41E2B7-5D3A-4F60-9B1E-7A2D6C9E4F15}_is1";

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

/// A published release newer than this build, with the download for this system and the checksum to verify it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub page: String,
    pub size: u64,
    asset_url: String,
    sum_url: String,
}

pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim().trim_start_matches('v').split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.split(['-', '+']).next()?.parse().ok()?;
    Some((major, minor, patch))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(format!("bawkbit/{VERSION}"))
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(|err| err.to_string())
}

fn describe(err: reqwest::Error) -> String {
    if err.is_timeout() {
        "it took too long to answer".into()
    } else if err.is_connect() {
        "cannot connect; check the internet connection".into()
    } else {
        err.without_url().to_string()
    }
}

async fn get(client: &reqwest::Client, url: &str) -> Result<reqwest::Response, String> {
    client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(describe)
}

fn pick(release: ApiRelease) -> Result<Release, String> {
    let download = release
        .assets
        .iter()
        .find(|asset| asset.name.ends_with(ASSET_SUFFIX))
        .ok_or("the newest release has no download for this system yet")?;
    let sum_name = format!("{}.sha256", download.name);
    let sum = release
        .assets
        .iter()
        .find(|asset| asset.name == sum_name)
        .ok_or("the newest release has no checksum to verify it with")?;
    Ok(Release {
        version: release.tag_name.trim_start_matches('v').to_string(),
        page: release.html_url,
        size: download.size,
        asset_url: download.browser_download_url.clone(),
        sum_url: sum.browser_download_url.clone(),
    })
}

/// A debug build can be pointed at a stand-in for GitHub, to try an update that was never published.
fn latest_api() -> String {
    #[cfg(debug_assertions)]
    if let Ok(api) = std::env::var("BAWKBIT_UPDATE_API") {
        return api;
    }
    LATEST_API.into()
}

/// The newest release on GitHub, when it is newer than this build.
pub async fn check() -> Result<Option<Release>, String> {
    check_at(&latest_api()).await
}

async fn check_at(api: &str) -> Result<Option<Release>, String> {
    let response = client()?
        .get(api)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|err| format!("could not reach GitHub: {}", describe(err)))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err("GitHub lists no published release of bawkbit yet".into());
    }
    let body = response
        .error_for_status()
        .map_err(|err| format!("could not reach GitHub: {}", describe(err)))?
        .bytes()
        .await
        .map_err(|err| format!("could not reach GitHub: {}", describe(err)))?;
    let release: ApiRelease = serde_json::from_slice(&body)
        .map_err(|err| format!("GitHub sent something unexpected: {err}"))?;
    if !is_newer(&release.tag_name, VERSION) {
        return Ok(None);
    }
    pick(release).map(Some)
}

/// The file an update replaces, unless this copy updates another way: a source build through git, a package through its package manager.
pub fn install_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let from_source = exe
        .parent()
        .and_then(|dir| Some((dir.file_name()?, dir.parent()?.file_name()?)))
        .is_some_and(|(profile, target)| {
            target == "target" && (profile == "debug" || profile == "release")
        });
    if from_source {
        return Err("this copy was built from source, so update it with git and cargo".into());
    }
    replaceable(exe)
}

#[cfg(windows)]
fn replaceable(exe: PathBuf) -> Result<PathBuf, String> {
    Ok(exe)
}

/// Only an AppImage replaces itself on Linux; its runtime names the file in `APPIMAGE`.
#[cfg(not(windows))]
fn replaceable(_exe: PathBuf) -> Result<PathBuf, String> {
    std::env::var_os("APPIMAGE").map(PathBuf::from).ok_or_else(|| {
        "only the AppImage updates itself. update this copy with your package manager, or download the new version.".into()
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Downloads and checks the release, then puts the new version where the running one is. It runs after a restart.
pub async fn install(release: &Release, progress: impl Fn(u64) + Send) -> Result<PathBuf, String> {
    let target = install_path()?;
    install_to(release, &target, progress).await?;
    Ok(target)
}

async fn install_to(
    release: &Release,
    target: &Path,
    progress: impl Fn(u64) + Send,
) -> Result<(), String> {
    let client = client()?;
    let sums = get(&client, &release.sum_url)
        .await
        .map_err(|err| format!("could not download the checksum: {err}"))?
        .text()
        .await
        .map_err(|err| format!("could not download the checksum: {}", describe(err)))?;
    let expected = sums
        .split_whitespace()
        .next()
        .ok_or("the checksum file is empty")?
        .to_lowercase();

    static DOWNLOADS: AtomicU32 = AtomicU32::new(0);
    let nth = DOWNLOADS.fetch_add(1, Ordering::Relaxed);
    let download =
        std::env::temp_dir().join(format!("bawkbit-{}-{nth}.update", std::process::id()));
    let fetched = fetch(&client, &release.asset_url, &download, &expected, progress).await;
    let placed = match fetched {
        Ok(()) => {
            let (from, to) = (download.clone(), target.to_path_buf());
            tokio::task::spawn_blocking(move || place(&from, &to))
                .await
                .unwrap_or_else(|err| Err(err.to_string()))
        }
        Err(err) => Err(err),
    };
    let _ = tokio::fs::remove_file(&download).await;
    placed
}

async fn fetch(
    client: &reqwest::Client,
    url: &str,
    path: &Path,
    expected: &str,
    progress: impl Fn(u64) + Send,
) -> Result<(), String> {
    let mut response = get(client, url)
        .await
        .map_err(|err| format!("could not download the update: {err}"))?;
    let mut file = tokio::fs::File::create(path)
        .await
        .map_err(|err| err.to_string())?;
    let mut hasher = Sha256::new();
    let mut done = 0u64;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| format!("the download broke off: {}", describe(err)))?
    {
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|err| err.to_string())?;
        done += chunk.len() as u64;
        progress(done);
    }
    file.flush().await.map_err(|err| err.to_string())?;
    if hex(&hasher.finalize()) != expected {
        return Err("the download does not match its checksum, so it was not installed".into());
    }
    Ok(())
}

#[cfg(windows)]
fn sibling(exe: &Path, tag: &str) -> PathBuf {
    let stem = exe.file_stem().unwrap_or_default().to_string_lossy();
    exe.with_file_name(format!("{stem}.{tag}.exe"))
}

/// The programs a Windows update replaces: the app always, and bawkbitd when this copy has it.
#[cfg(windows)]
fn programs(exe: &Path) -> Vec<(&'static str, PathBuf)> {
    let daemon = exe.with_file_name("bawkbitd.exe");
    let mut programs = vec![("bawkbit.exe", exe.to_path_buf())];
    if daemon.exists() {
        programs.push(("bawkbitd.exe", daemon));
    }
    programs
}

#[cfg(windows)]
fn place(download: &Path, exe: &Path) -> Result<(), String> {
    let programs = programs(exe);
    let result = extract_all(download, &programs).and_then(|()| swap_all(&programs));
    for (_, target) in &programs {
        let _ = fs::remove_file(sibling(target, "new"));
    }
    result
}

#[cfg(windows)]
fn extract_all(zip_path: &Path, programs: &[(&str, PathBuf)]) -> Result<(), String> {
    let file = fs::File::open(zip_path).map_err(|err| err.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|err| err.to_string())?;
    for (name, target) in programs {
        let index = (0..archive.len())
            .find(|&ix| {
                archive
                    .name_for_index(ix)
                    .is_some_and(|entry| entry.rsplit(['/', '\\']).next() == Some(*name))
            })
            .ok_or_else(|| format!("the download has no {name} in it"))?;
        let mut entry = archive.by_index(index).map_err(|err| err.to_string())?;
        let mut out = fs::File::create(sibling(target, "new")).map_err(|err| {
            format!("cannot write next to {name} ({err}). download the new version by hand.")
        })?;
        std::io::copy(&mut entry, &mut out).map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// Windows lets a running exe be renamed but not overwritten, so each old one steps aside first; a failure puts back those already swapped.
#[cfg(windows)]
fn swap_all(programs: &[(&str, PathBuf)]) -> Result<(), String> {
    for (done, (name, target)) in programs.iter().enumerate() {
        if let Err(err) = swap(target, &sibling(target, "new")) {
            for (_, back) in programs[..done].iter().rev() {
                let _ = fs::remove_file(back);
                let _ = fs::rename(sibling(back, "old"), back);
            }
            return Err(format!("cannot replace {name}: {err}"));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn swap(exe: &Path, fresh: &Path) -> std::io::Result<()> {
    let old = sibling(exe, "old");
    let _ = fs::remove_file(&old);
    fs::rename(exe, &old)?;
    if let Err(err) = fs::rename(fresh, exe) {
        let _ = fs::rename(&old, exe);
        return Err(err);
    }
    Ok(())
}

/// Linux lets a running file be renamed over, so the new AppImage lands in one step.
#[cfg(not(windows))]
fn place(download: &Path, appimage: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let fresh = appimage.with_extension("new");
    let result = fs::copy(download, &fresh)
        .and_then(|_| fs::set_permissions(&fresh, fs::Permissions::from_mode(0o755)))
        .and_then(|()| fs::rename(&fresh, appimage))
        .map_err(|err| {
            format!("cannot write next to the AppImage ({err}). download the new version by hand.")
        });
    let _ = fs::remove_file(&fresh);
    result
}

/// Starts the freshly installed version; the caller quits this one.
pub fn relaunch(path: &Path) -> Result<(), String> {
    Command::new(path)
        .arg(AFTER_UPDATE)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("could not start the new version: {err}"))
}

/// Removes the programs an earlier update set aside, waiting for the copies still running them to quit.
#[cfg(windows)]
pub fn clean_up() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut leftovers: Vec<PathBuf> = [exe.clone(), exe.with_file_name("bawkbitd.exe")]
        .iter()
        .map(|program| sibling(program, "old"))
        .filter(|old| old.exists())
        .collect();
    if leftovers.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        for _ in 0..30 {
            leftovers.retain(|old| fs::remove_file(old).is_err() && old.exists());
            if leftovers.is_empty() {
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

/// An update swaps the programs without the installer, so the version Windows lists for an installed copy is corrected here.
#[cfg(windows)]
pub fn sync_installed_version() {
    if let Ok(exe) = std::env::current_exe() {
        sync_version(UNINSTALL_KEY, &exe, VERSION);
    }
}

#[cfg(windows)]
fn sync_version(key: &str, exe: &Path, version: &str) {
    use windows_registry::CURRENT_USER;
    let Ok(entry) = CURRENT_USER.options().read().write().open(key) else {
        return;
    };
    let location = entry.get_string("InstallLocation").unwrap_or_default();
    let ours = exe.parent().is_some_and(|dir| {
        dir.to_string_lossy()
            .eq_ignore_ascii_case(location.trim_end_matches('\\'))
    });
    let shown = entry.get_string("DisplayVersion").unwrap_or_default();
    let Some((major, minor, _)) = parse_version(version).filter(|_| ours && shown != version)
    else {
        return;
    };
    let _ = entry.set_string("DisplayVersion", version);
    for (name, value) in [
        ("MajorVersion", major),
        ("VersionMajor", major),
        ("MinorVersion", minor),
        ("VersionMinor", minor),
    ] {
        let _ = entry.set_u32(name, value as u32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    fn asset(name: &str) -> ApiAsset {
        ApiAsset {
            name: name.into(),
            browser_download_url: format!("https://example.com/{name}"),
            size: 10,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bawkbit-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The download a release carries for this system, holding `app` as the app and `daemon` as bawkbitd.
    #[cfg(windows)]
    fn package(app: &[u8], daemon: &[u8]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in [("bawkbit.exe", app), ("bawkbitd.exe", daemon)] {
            writer
                .start_file(
                    format!("bawkbit-v99.0.0-windows-x64/{name}"),
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[cfg(not(windows))]
    fn package(app: &[u8], _daemon: &[u8]) -> Vec<u8> {
        app.to_vec()
    }

    /// A stand-in for GitHub: answers each path with the bytes given, and 404 for the rest.
    fn serve(listener: TcpListener, files: Vec<(String, Vec<u8>)>) {
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut request = Vec::new();
                let mut byte = [0u8; 1];
                while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                    request.push(byte[0]);
                }
                let request = String::from_utf8_lossy(&request).into_owned();
                let path = request.split_whitespace().nth(1).unwrap_or_default();
                let found = files.iter().find(|(name, _)| name == path);
                let (status, body) = match found {
                    Some((_, body)) => ("200 OK", body.as_slice()),
                    None => ("404 Not Found", &b""[..]),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body);
            }
        });
    }

    /// Publishes release 99.0.0 on a local server; `sum` overrides the checksum it advertises.
    fn publish(app: &[u8], sum: Option<&str>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let name = format!("bawkbit-v99.0.0{ASSET_SUFFIX}");
        let download = package(app, b"the new daemon");
        let sum = sum.map_or_else(|| hex(&Sha256::digest(&download)), str::to_string);
        let latest = format!(
            r#"{{"tag_name":"v99.0.0","html_url":"{base}/notes","assets":[
                {{"name":"{name}","browser_download_url":"{base}/download","size":{}}},
                {{"name":"{name}.sha256","browser_download_url":"{base}/sum","size":90}}]}}"#,
            download.len()
        );
        let files = vec![
            ("/latest".to_string(), latest.into_bytes()),
            ("/download".to_string(), download),
            ("/sum".to_string(), format!("{sum}  {name}\n").into_bytes()),
        ];
        serve(listener, files);
        base
    }

    #[test]
    fn compares_versions() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.10.0-beta.1"), Some((0, 10, 0)));
        assert_eq!(parse_version("nonsense"), None);
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(is_newer("v0.10.0", "0.9.0"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("garbage", "0.1.0"));
    }

    #[test]
    fn picks_this_systems_download_and_its_checksum() {
        let names = [
            "bawkbit-0.2.0-1.x86_64.rpm",
            "bawkbit-0.2.0-1.x86_64.rpm.sha256",
            "bawkbit-v0.2.0-linux-x64.AppImage",
            "bawkbit-v0.2.0-linux-x64.AppImage.sha256",
            "bawkbit-v0.2.0-linux-x64.tar.gz",
            "bawkbit-v0.2.0-windows-x64-setup.exe",
            "bawkbit-v0.2.0-windows-x64-setup.exe.sha256",
            "bawkbit-v0.2.0-windows-x64.zip",
            "bawkbit-v0.2.0-windows-x64.zip.sha256",
            "bawkbitd-v0.2.0-linux-x64.tar.gz",
            "bawkbit_0.2.0-1_amd64.deb",
        ];
        let ours = format!("bawkbit-v0.2.0{ASSET_SUFFIX}");
        let release = ApiRelease {
            tag_name: "v0.2.0".into(),
            html_url: "https://example.com/v0.2.0".into(),
            assets: names.into_iter().map(asset).collect(),
        };
        let picked = pick(release).unwrap();
        assert_eq!(picked.version, "0.2.0");
        assert!(picked.asset_url.ends_with(&format!("/{ours}")));
        assert!(picked.sum_url.ends_with(&format!("/{ours}.sha256")));
        let bare = ApiRelease {
            tag_name: "v0.2.0".into(),
            html_url: String::new(),
            assets: vec![asset(&ours)],
        };
        assert!(pick(bare).is_err());
    }

    #[test]
    fn an_update_is_found_downloaded_checked_and_put_in_place() {
        let base = publish(b"the new app", None);
        let release = block_on(check_at(&format!("{base}/latest")))
            .unwrap()
            .unwrap();
        assert_eq!(release.version, "99.0.0");
        assert_eq!(release.page, format!("{base}/notes"));

        let dir = scratch("install");
        let target = dir.join(if cfg!(windows) {
            "bawkbit.exe"
        } else {
            "bawkbit.AppImage"
        });
        fs::write(&target, b"the old app").unwrap();
        #[cfg(windows)]
        fs::write(dir.join("bawkbitd.exe"), b"the old daemon").unwrap();
        let seen = std::sync::atomic::AtomicU64::new(0);
        block_on(install_to(&release, &target, |bytes| {
            seen.store(bytes, Ordering::Relaxed)
        }))
        .unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"the new app");
        assert_eq!(seen.load(Ordering::Relaxed), release.size);
        #[cfg(windows)]
        {
            assert_eq!(
                fs::read(dir.join("bawkbitd.exe")).unwrap(),
                b"the new daemon"
            );
            assert_eq!(
                fs::read(dir.join("bawkbit.old.exe")).unwrap(),
                b"the old app"
            );
            assert_eq!(fs::read_dir(&dir).unwrap().count(), 4);
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111);
            assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_download_that_fails_its_checksum_is_not_installed() {
        let base = publish(b"tampered", Some(&"0".repeat(64)));
        let release = block_on(check_at(&format!("{base}/latest")))
            .unwrap()
            .unwrap();
        let dir = scratch("checksum");
        let target = dir.join("bawkbit.exe");
        fs::write(&target, b"the old app").unwrap();
        let error = block_on(install_to(&release, &target, |_| {})).unwrap_err();
        assert!(error.contains("checksum"), "{error}");
        assert_eq!(fs::read(&target).unwrap(), b"the old app");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    #[ignore = "downloads the newest release from GitHub"]
    fn installs_the_newest_release_from_github() {
        assert!(block_on(check()).is_ok());
        let release = block_on(async {
            let client = client().unwrap();
            let body = get(&client, LATEST_API).await.unwrap().bytes().await;
            pick(serde_json::from_slice(&body.unwrap()).unwrap()).unwrap()
        });
        let dir = scratch("github");
        let target = dir.join(if cfg!(windows) {
            "bawkbit.exe"
        } else {
            "bawkbit.AppImage"
        });
        fs::write(&target, b"old").unwrap();
        #[cfg(windows)]
        fs::write(dir.join("bawkbitd.exe"), b"old").unwrap();
        block_on(install_to(&release, &target, |_| {})).unwrap();
        assert!(fs::metadata(&target).unwrap().len() > 1_000_000);
        #[cfg(windows)]
        assert!(fs::metadata(dir.join("bawkbitd.exe")).unwrap().len() > 1_000_000);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_release_and_an_old_one_are_told_apart() {
        let base = publish(b"app", None);
        let error = block_on(check_at(&format!("{base}/nothing-here"))).unwrap_err();
        assert!(error.contains("no published release"), "{error}");

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let api = format!("http://{}/latest", listener.local_addr().unwrap());
        let same = format!(r#"{{"tag_name":"v{VERSION}","html_url":"","assets":[]}}"#);
        serve(listener, vec![("/latest".to_string(), same.into_bytes())]);
        assert_eq!(block_on(check_at(&api)), Ok(None));
    }

    #[cfg(windows)]
    #[test]
    fn a_failed_swap_puts_back_the_programs_already_swapped() {
        let dir = scratch("rollback");
        let app = dir.join("bawkbit.exe");
        let daemon = dir.join("bawkbitd.exe");
        fs::write(&app, b"old app").unwrap();
        fs::write(&daemon, b"old daemon").unwrap();
        fs::write(sibling(&app, "new"), b"new app").unwrap();
        fs::write(sibling(&daemon, "new"), b"new daemon").unwrap();
        let blocker = sibling(&daemon, "old");
        fs::create_dir_all(blocker.join("in-use")).unwrap();

        let error = swap_all(&programs(&app)).unwrap_err();
        assert!(error.contains("bawkbitd.exe"), "{error}");
        assert_eq!(fs::read(&app).unwrap(), b"old app");
        assert_eq!(fs::read(&daemon).unwrap(), b"old daemon");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn the_listed_version_is_corrected_only_for_this_copy() {
        use windows_registry::CURRENT_USER;
        let key = format!(r"Software\bawkbit-test-updater-{}", std::process::id());
        let entry = CURRENT_USER.create(&key).unwrap();
        entry
            .set_string("InstallLocation", r"C:\Apps\bawkbit\")
            .unwrap();
        entry.set_string("DisplayVersion", "0.1.0").unwrap();
        entry.set_u32("VersionMinor", 1).unwrap();

        sync_version(&key, Path::new(r"D:\elsewhere\bawkbit.exe"), "0.3.0");
        assert_eq!(entry.get_string("DisplayVersion").unwrap(), "0.1.0");
        sync_version(&key, Path::new(r"c:\apps\bawkbit\bawkbit.exe"), "0.3.0");
        assert_eq!(entry.get_string("DisplayVersion").unwrap(), "0.3.0");
        assert_eq!(entry.get_u32("VersionMinor").unwrap(), 3);
        CURRENT_USER.remove_tree(&key).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn the_uninstall_key_and_the_names_match_the_installer() {
        let script = include_str!("../packaging/windows/bawkbit.iss");
        let id = UNINSTALL_KEY
            .rsplit('\\')
            .next()
            .unwrap()
            .trim_end_matches("_is1");
        assert!(script.contains(&format!("AppId={{{id}")));
        for leftover in ["bawkbit.old.exe", "bawkbitd.old.exe"] {
            assert!(
                script.contains(&format!(r"{{app}}\{leftover}")),
                "{leftover}"
            );
        }
        let package = include_str!("../packaging/windows/package.ps1");
        assert!(package.contains("\"bawkbit-v$version-windows-x64\""));
        assert!(package.contains("\"dist/$name.zip\""));
    }

    #[cfg(not(windows))]
    #[test]
    fn the_names_match_the_linux_packaging() {
        let package = include_str!("../packaging/linux/package.sh");
        assert!(package.contains("name=\"bawkbit-$tag-linux-x64\""));
        assert!(package.contains("\"$out/$name.AppImage\""));
    }
}
