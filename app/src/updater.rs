//! Download cache path and macOS `.app` install from a release zip.

use std::path::PathBuf;

#[cfg(target_os = "macos")]
use std::path::Path;

use chm_update::ReleaseInfo;

/// `~/Library/Caches/chmonitor/updates` (macOS) or the platform cache dir.
pub fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("chmonitor").join("updates"))
}

pub fn archive_path(release: &ReleaseInfo) -> Option<PathBuf> {
    let name = release
        .url()
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("chmonitor-update.bin");
    cache_dir().map(|d| {
        d.join(format!("v{name}", name = release.version()))
            .join(name)
    })
}

/// If this process is running from `chmonitor.app/Contents/MacOS/…`,
/// return the `.app` bundle path.
pub fn macos_bundle_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let macos = exe.parent()?;
    if macos.file_name()?.to_str()? != "MacOS" {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name()?.to_str()? != "Contents" {
        return None;
    }
    let bundle = contents.parent()?;
    if bundle.extension()?.to_str()? != "app" {
        return None;
    }
    Some(bundle.to_path_buf())
}

/// Unpack a macOS zip (`.app` inside) and swap it over the running bundle.
/// Returns the installed `.app` path. Caller should relaunch and quit.
#[cfg(target_os = "macos")]
pub fn install_macos_zip(zip: &Path) -> Result<PathBuf, String> {
    let bundle = macos_bundle_path().ok_or_else(|| {
        "not running from chmonitor.app — open the zip from Downloads".to_string()
    })?;
    let parent = bundle
        .parent()
        .ok_or_else(|| "app bundle has no parent directory".to_string())?;
    let stage = parent.join(format!(".chmonitor-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(&stage).map_err(|e| format!("stage mkdir: {e}"))?;
    let status = std::process::Command::new("/usr/bin/ditto")
        .args(["-xk", "--"])
        .arg(zip)
        .arg(&stage)
        .status()
        .map_err(|e| format!("ditto: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(format!("ditto exited {status}"));
    }
    let fresh = find_app(&stage).ok_or_else(|| {
        let _ = std::fs::remove_dir_all(&stage);
        "zip did not contain a .app bundle".to_string()
    })?;
    let backup = parent.join(format!("chmonitor.app.bak-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&backup);
    std::fs::rename(&bundle, &backup).map_err(|e| {
        let _ = std::fs::remove_dir_all(&stage);
        format!("move running app aside: {e}")
    })?;
    let dest = parent.join("chmonitor.app");
    if let Err(e) = std::fs::rename(&fresh, &dest) {
        let _ = std::fs::rename(&backup, &bundle);
        let _ = std::fs::remove_dir_all(&stage);
        return Err(format!("install new app: {e}"));
    }
    let _ = std::fs::remove_dir_all(&backup);
    let _ = std::fs::remove_dir_all(&stage);
    Ok(dest)
}

#[cfg(target_os = "macos")]
fn find_app(root: &Path) -> Option<PathBuf> {
    let mut found = None;
    let walker = std::fs::read_dir(root).ok()?;
    for entry in walker.flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) == Some("app") {
            return Some(p);
        }
        if p.is_dir()
            && let Some(inner) = find_app(&p)
        {
            found = Some(inner);
        }
    }
    found
}

#[cfg(target_os = "macos")]
pub fn relaunch(app: &Path) {
    let _ = std::process::Command::new("/usr/bin/open").arg(app).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_path_nests_under_version() {
        let release = serde_json::from_str::<chm_update::ReleaseInfo>(
            r#"{"version":"1.2.3","url":"https://x/chmonitor-v1.2.3.zip","notes":""}"#,
        )
        .unwrap();
        let path = archive_path(&release).expect("cache dir");
        assert!(path.ends_with("v1.2.3/chmonitor-v1.2.3.zip"));
    }
}
