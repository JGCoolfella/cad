//! The native file dialog (G1a): assemblies refer to their parts by path, which the
//! webview's file pickers do not reveal. Windows' common Open and Save As dialogs
//! (`GetOpenFileNameW`, `GetSaveFileNameW`); other systems have none yet.
use std::path::PathBuf;

/// File types offered, by name: `part`, `assembly`, `step`, `stl`, or `open` (all that
/// File > Open reads).
fn filter(kinds: &[String]) -> Vec<(&'static str, &'static str)> {
    let mut out = vec![];
    for k in kinds {
        match k.as_str() {
            "open" => out.push(("OpenCAD Files (*.ocpart; *.ocasm; *.step; *.stp)", "*.ocpart;*.ocasm;*.step;*.stp")),
            "part" => out.push(("OpenCAD Part (*.ocpart)", "*.ocpart")),
            "assembly" => out.push(("OpenCAD Assembly (*.ocasm)", "*.ocasm")),
            "step" => out.push(("STEP AP203/214/242 (*.step; *.stp)", "*.step;*.stp")),
            "stl" => out.push(("STL (*.stl)", "*.stl")),
            _ => {}
        }
    }
    out
}

/// Shows the dialog over `owner` (a window handle, 0 for none) and returns the chosen
/// path, or `None` when cancelled.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn pick(owner: isize, save: bool, kinds: &[String], name: Option<&str>) -> Result<Option<PathBuf>, String> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::UI::Controls::Dialogs::*;
    let wide = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
    let mut filt: Vec<u16> = vec![];
    for (d, p) in filter(kinds) {
        filt.extend(wide(d));
        filt.push(0);
        filt.extend(wide(p));
        filt.push(0);
    }
    filt.push(0);
    let mut file = vec![0u16; 4096];
    if let Some(n) = name {
        let w = wide(n);
        let k = w.len().min(4000);
        file[..k].copy_from_slice(&w[..k]);
    }
    let ext = match kinds.first().map(String::as_str) {
        Some("assembly") => "ocasm",
        Some("step") => "step",
        Some("stl") => "stl",
        _ => "ocpart",
    };
    let mut def_ext = wide(ext);
    def_ext.push(0);
    // SAFETY: every pointer in the struct points into a buffer that outlives the call, and
    // the file buffer's length is given in `nMaxFile`.
    unsafe {
        let mut o: OPENFILENAMEW = std::mem::zeroed();
        o.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        o.hwndOwner = owner as _;
        o.lpstrFilter = filt.as_ptr();
        o.nFilterIndex = 1;
        o.lpstrFile = file.as_mut_ptr();
        o.nMaxFile = file.len() as u32;
        o.lpstrDefExt = def_ext.as_ptr();
        o.Flags = OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | if save { OFN_OVERWRITEPROMPT } else { OFN_FILEMUSTEXIST };
        let ok = if save { GetSaveFileNameW(&mut o) } else { GetOpenFileNameW(&mut o) };
        if ok == 0 {
            let e = CommDlgExtendedError();
            return if e == 0 { Ok(None) } else { Err(format!("file dialog error {e:#x}")) };
        }
    }
    let len = file.iter().position(|&c| c == 0).unwrap_or(file.len());
    Ok(Some(PathBuf::from(std::ffi::OsString::from_wide(&file[..len]))))
}

#[cfg(not(windows))]
pub fn pick(_owner: isize, _save: bool, kinds: &[String], _name: Option<&str>) -> Result<Option<PathBuf>, String> {
    let _ = filter(kinds);
    Err("the file dialog is not available on this system yet".into())
}
