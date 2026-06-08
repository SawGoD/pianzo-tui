//! Проверка и установка обновлений через GitHub Releases.

use std::sync::mpsc::Sender;
use std::thread;

#[derive(Debug)]
pub enum UpdateMsg {
    Available(String),
    UpToDate,
    Downloading,
    Done,
    Error(String),
}

fn platform_target() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "silicon-apple"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "intel-apple"
    } else if cfg!(target_os = "windows") {
        "x86_64-windows"
    } else {
        "unknown"
    }
}

pub fn spawn_check(tx: Sender<UpdateMsg>) {
    thread::spawn(move || match check_latest() {
        Ok(Some(v)) => { let _ = tx.send(UpdateMsg::Available(v)); }
        Ok(None) => { let _ = tx.send(UpdateMsg::UpToDate); }
        Err(e) => { let _ = tx.send(UpdateMsg::Error(e)); }
    });
}

fn check_latest() -> Result<Option<String>, String> {
    use self_update::backends::github::ReleaseList;
    let releases = ReleaseList::configure()
        .repo_owner("SawGoD")
        .repo_name("pianzo-tui")
        .build()
        .map_err(|e| e.to_string())?
        .fetch()
        .map_err(|e| e.to_string())?;

    let Some(latest) = releases.first() else {
        return Ok(None);
    };

    let current = env!("CARGO_PKG_VERSION");
    match self_update::version::bump_is_greater(current, &latest.version) {
        Ok(true) => Ok(Some(latest.version.clone())),
        _ => Ok(None),
    }
}

pub fn spawn_update(tx: Sender<UpdateMsg>) {
    thread::spawn(move || {
        let _ = tx.send(UpdateMsg::Downloading);
        match perform_update() {
            Ok(ver) => {
                crate::debug::log(&format!("updater: обновлено до {ver}"));
                let _ = tx.send(UpdateMsg::Done);
            }
            Err(e) => {
                crate::debug::log(&format!("updater: ошибка — {e}"));
                let _ = tx.send(UpdateMsg::Error(e));
            }
        }
    });
}

fn perform_update() -> Result<String, String> {
    use self_update::backends::github::Update;
    let status = Update::configure()
        .repo_owner("SawGoD")
        .repo_name("pianzo-tui")
        .bin_name("pianzo-tui")
        .target(platform_target())
        .current_version(env!("CARGO_PKG_VERSION"))
        .no_confirm(true)
        .show_download_progress(false)
        .show_output(false)
        .build()
        .map_err(|e| e.to_string())?
        .update()
        .map_err(|e| e.to_string())?;

    Ok(status.version().to_string())
}
