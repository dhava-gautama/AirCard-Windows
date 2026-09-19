//! Detect Apple desktop apps that steal the AirTraffic session.

pub fn blocking_sync_apps() -> Vec<String> {
    let mut hits = Vec::new();
    for name in running_process_names() {
        let lower = name.to_ascii_lowercase();
        if lower == "itunes.exe"
            || lower == "appledevices.exe"
            || lower == "applemusic.exe"
            || lower == "ampdevicesagent.exe"
            || lower == "applemobiledevicehelper.exe"
        {
            if !hits.iter().any(|h: &String| h.eq_ignore_ascii_case(&name)) {
                hits.push(name);
            }
        }
    }
    hits
}

#[cfg(windows)]
fn running_process_names() -> Vec<String> {
    #[repr(C)]
    struct ProcessEntry32W {
        dw_size: u32,
        cnt_usage: u32,
        th32_process_id: u32,
        th32_default_heap_id: usize,
        th32_module_id: u32,
        cnt_threads: u32,
        th32_parent_process_id: u32,
        pc_pri_class_base: i32,
        dw_flags: u32,
        sz_exe_file: [u16; 260],
    }

    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(dwFlags: u32, th32ProcessID: u32) -> isize;
        fn Process32FirstW(hSnapshot: isize, lppe: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(hSnapshot: isize, lppe: *mut ProcessEntry32W) -> i32;
        fn CloseHandle(hObject: isize) -> i32;
    }

    const TH32CS_SNAPPROCESS: u32 = 0x2;
    const INVALID: isize = -1;

    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == 0 || snap == INVALID {
        return Vec::new();
    }

    let mut entry = ProcessEntry32W {
        dw_size: std::mem::size_of::<ProcessEntry32W>() as u32,
        cnt_usage: 0,
        th32_process_id: 0,
        th32_default_heap_id: 0,
        th32_module_id: 0,
        cnt_threads: 0,
        th32_parent_process_id: 0,
        pc_pri_class_base: 0,
        dw_flags: 0,
        sz_exe_file: [0; 260],
    };

    let mut names = Vec::new();
    unsafe {
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                let len = entry
                    .sz_exe_file
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.sz_exe_file.len());
                names.push(String::from_utf16_lossy(&entry.sz_exe_file[..len]));
                entry.sz_exe_file = [0; 260];
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    names
}

#[cfg(not(windows))]
fn running_process_names() -> Vec<String> {
    Vec::new()
}
