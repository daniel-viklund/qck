use super::{is_executable_name, walk};
use std::{
    collections::HashSet,
    ffi::OsString,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    ptr::{null, null_mut},
};
use windows::{
    Win32::{
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize, IPersistFile, STGM_READ,
        },
        UI::Shell::{
            FOLDERID_CommonPrograms, FOLDERID_Programs, IShellLinkW, KF_FLAG_DEFAULT,
            SHGetKnownFolderPath, ShellLink,
        },
    },
    core::{Interface, PCWSTR},
};
use windows_sys::Win32::{
    Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS},
    Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives},
    System::{Registry::*, Threading::*},
};

pub(super) fn scan(mut record: impl FnMut(PathBuf)) {
    // Lower disk/CPU priority as well as keeping discovery off the action worker.
    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) };
    let mut roots = Vec::new();
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            for key in subkeys(
                hive,
                r"Software\Microsoft\Windows\CurrentVersion\App Paths",
                view,
            ) {
                if let Some(path) = registry_path(&key, None) {
                    record(path);
                }
            }
            for key in subkeys(
                hive,
                r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
                view,
            ) {
                if let Some(path) = registry_path(&key, Some("InstallLocation")) {
                    if path.is_file() {
                        record(path.clone());
                        if let Some(parent) = path.parent() {
                            roots.push(parent.to_path_buf());
                        }
                    } else if path.is_absolute() {
                        roots.push(path);
                    }
                }
            }
        }
    }
    start_menu_targets(&mut record);
    // PATH entries only need a shallow scan. Do not recursively index everything
    // under a PATH entry like C:\ before checking normal installation locations.
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path).filter(|p| p.is_absolute()) {
            if let Ok(entries) = std::fs::read_dir(directory) {
                for entry in entries.flatten() {
                    if entry.file_name().to_str().is_some_and(is_executable_name) {
                        record(entry.path());
                    }
                }
            }
        }
    }
    if let Some(local) = environment_path("LOCALAPPDATA") {
        roots.push(local.join("Programs"));
    }
    for variable in [
        "ProgramW6432",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "APPDATA",
        "LOCALAPPDATA",
    ] {
        if let Some(path) = environment_path(variable) {
            roots.push(path);
        }
    }
    let mut visited = HashSet::new();
    walk(roots, &mut visited, |path| {
        if path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(is_executable_name)
        {
            record(path);
        }
    });
    // Find portable/custom apps too, after installed apps have priority. Only
    // mounted fixed drives are searched; junctions and symlinks are not followed.
    eprintln!("qck: searching local fixed drives for additional application paths");
    let drives = fixed_drives();
    walk(drives, &mut visited, |path| {
        if path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(is_executable_name)
        {
            record(path);
        }
    });
}

fn environment_path(variable: &str) -> Option<PathBuf> {
    std::env::var_os(variable)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

fn fixed_drives() -> Vec<PathBuf> {
    let mask = unsafe { GetLogicalDrives() };
    (0..26)
        .filter(|letter| mask & (1 << letter) != 0)
        .filter_map(|letter| {
            let root = format!("{}:\\", (b'A' + letter) as char);
            let wide: Vec<_> = root.encode_utf16().chain(Some(0)).collect();
            // DRIVE_FIXED (3); exclude removable, optical, and mapped network drives.
            (unsafe { GetDriveTypeW(wide.as_ptr()) } == 3).then(|| PathBuf::from(root))
        })
        .collect()
}

struct RegistryKey(HKEY);
impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

fn subkeys(hive: HKEY, path: &str, view: REG_SAM_FLAGS) -> Vec<RegistryKey> {
    let path: Vec<_> = path.encode_utf16().chain(Some(0)).collect();
    let mut parent = null_mut();
    if unsafe { RegOpenKeyExW(hive, path.as_ptr(), 0, KEY_READ | view, &mut parent) }
        != ERROR_SUCCESS
    {
        return Vec::new();
    }
    let parent = RegistryKey(parent);
    let mut keys = Vec::new();
    for index in 0.. {
        let mut name = [0u16; 256];
        let mut length = name.len() as u32;
        let result = unsafe {
            RegEnumKeyExW(
                parent.0,
                index,
                name.as_mut_ptr(),
                &mut length,
                null(),
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        if result == ERROR_NO_MORE_ITEMS {
            break;
        }
        if result != ERROR_SUCCESS {
            break;
        }
        let mut child = null_mut();
        if unsafe { RegOpenKeyExW(parent.0, name.as_ptr(), 0, KEY_READ | view, &mut child) }
            == ERROR_SUCCESS
        {
            keys.push(RegistryKey(child));
        }
    }
    keys
}

fn registry_path(key: &RegistryKey, value: Option<&str>) -> Option<PathBuf> {
    let value = value.map(|s| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>());
    let mut buffer = vec![0u16; 32768];
    let mut bytes = (buffer.len() * 2) as u32;
    let result = unsafe {
        RegGetValueW(
            key.0,
            null(),
            value.as_ref().map_or(null(), |v| v.as_ptr()),
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
            null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if result != ERROR_SUCCESS {
        return None;
    }
    let length = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    let mut text = &buffer[..length];
    if text.len() >= 2 && text[0] == 34 && text[text.len() - 1] == 34 {
        text = &text[1..text.len() - 1];
    }
    (!text.is_empty()).then(|| PathBuf::from(OsString::from_wide(text)))
}

struct ComApartment;
impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

fn start_menu_targets(record: &mut impl FnMut(PathBuf)) {
    if unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_err() {
        return;
    }
    let _apartment = ComApartment;
    for folder in [FOLDERID_Programs, FOLDERID_CommonPrograms] {
        let Ok(raw) = (unsafe { SHGetKnownFolderPath(&folder, KF_FLAG_DEFAULT, None) }) else {
            continue;
        };
        let path = PathBuf::from(OsString::from_wide(unsafe { raw.as_wide() }));
        unsafe { CoTaskMemFree(Some(raw.0.cast())) };
        walk([path], &mut HashSet::new(), |path| {
            if path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("lnk"))
                && let Some(target) = shortcut_target(&path)
            {
                record(target);
            }
        });
    }
}

fn shortcut_target(path: &Path) -> Option<PathBuf> {
    let path: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut target = vec![0u16; 32768];
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        file.Load(PCWSTR(path.as_ptr()), STGM_READ).ok()?;
        // Read the existing target without Resolve(), which can search networks
        // or update a shortcut. Discovery never launches or rewrites shortcuts.
        link.GetPath(&mut target, null_mut(), 0).ok()?;
    }
    let length = target.iter().position(|c| *c == 0)?;
    (length > 0).then(|| PathBuf::from(OsString::from_wide(&target[..length])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_start_menu_style_shortcut_without_launching_its_target() {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .unwrap();
        let _apartment = ComApartment;
        let root = std::env::temp_dir().join(format!("qck-shortcut-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("Bitwarden.exe");
        let shortcut = root.join("Password Manager.lnk");
        std::fs::write(&target, []).unwrap();
        let target_wide: Vec<_> = target.as_os_str().encode_wide().chain(Some(0)).collect();
        let shortcut_wide: Vec<_> = shortcut.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            link.SetPath(PCWSTR(target_wide.as_ptr())).unwrap();
            let file: IPersistFile = link.cast().unwrap();
            file.Save(PCWSTR(shortcut_wide.as_ptr()), true).unwrap();
        }
        let actual = shortcut_target(&shortcut);
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(actual, Some(target));
    }
}
