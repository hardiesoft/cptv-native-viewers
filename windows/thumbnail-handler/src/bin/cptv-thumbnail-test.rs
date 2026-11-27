//! Test tool for the Explorer thumbnail handler (not shipped).
//!
//!   cptv-thumbnail-test direct <cptv_thumbnail_handler.dll> <file.cptv> <size> <out.bmp>
//!       Load the DLL and call it the way Explorer does (DllGetClassObject →
//!       IInitializeWithStream → IThumbnailProvider), without any
//!       registration. Works under Wine.
//!   cptv-thumbnail-test shell <file.cptv> <size> <out.bmp>
//!       Ask Windows' own thumbnail pipeline (IShellItemImageFactory, thumbnails
//!       only, no icon fallback), which only succeeds if the handler is
//!       registered for .cptv and works in Explorer's isolated host process.
//!
//! Writes the bitmap as a .bmp and fails unless it's a plausible picture
//! (expected size, more than a handful of colours).

use std::collections::HashSet;
use std::ffi::c_void;
use std::process::ExitCode;

use windows::core::{s, Interface, HSTRING, GUID, HRESULT};
use windows::Win32::Foundation::{FreeLibrary, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
};
use windows::Win32::System::Com::{
    CoInitializeEx, IClassFactory, COINIT_APARTMENTTHREADED, STGM_READ, STGM_SHARE_DENY_NONE,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Shell::PropertiesSystem::IInitializeWithStream;
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, IThumbnailProvider, SHCreateItemFromParsingName,
    SHCreateStreamOnFileEx, SIIGBF_THUMBNAILONLY, WTS_ALPHATYPE,
};

const CLSID_CPTV_THUMBNAIL: GUID = GUID::from_u128(0xFBF13CFA_0D80_4899_926E_C50711CDFA5A);

type DllGetClassObjectFn =
    unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;

fn usage() -> ExitCode {
    eprintln!(
        "usage: cptv-thumbnail-test direct <handler.dll> <file.cptv> <size> <out.bmp>\n       \
         cptv-thumbnail-test shell <file.cptv> <size> <out.bmp>"
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["direct", dll, file, size, out] => size
            .parse()
            .map_err(|_| "bad size".to_string())
            .and_then(|size| direct(dll, file, size).and_then(|b| save(b, size, out))),
        ["shell", file, size, out] => size
            .parse()
            .map_err(|_| "bad size".to_string())
            .and_then(|size| shell(file, size).and_then(|b| save(b, size, out))),
        _ => return usage(),
    };
    match result {
        Ok(msg) => {
            println!("OK: {msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("FAIL: {e}");
            ExitCode::FAILURE
        }
    }
}

fn err(what: &str) -> impl Fn(windows::core::Error) -> String + '_ {
    move |e| format!("{what}: {e}")
}

fn direct(dll: &str, file: &str, size: u32) -> Result<HBITMAP, String> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().map_err(err("CoInitializeEx"))?;
        let module = LoadLibraryW(&HSTRING::from(dll)).map_err(err("LoadLibrary"))?;
        let proc = GetProcAddress(module, s!("DllGetClassObject"))
            .ok_or("DllGetClassObject not exported")?;
        let get_class: DllGetClassObjectFn = std::mem::transmute(proc);

        let mut factory: *mut c_void = std::ptr::null_mut();
        get_class(&CLSID_CPTV_THUMBNAIL, &IClassFactory::IID, &mut factory)
            .ok()
            .map_err(err("DllGetClassObject"))?;
        let factory = IClassFactory::from_raw(factory);
        let init: IInitializeWithStream =
            factory.CreateInstance(None).map_err(err("CreateInstance"))?;

        let stream = SHCreateStreamOnFileEx(
            &HSTRING::from(file),
            (STGM_READ | STGM_SHARE_DENY_NONE).0,
            0,
            false,
            None,
        )
        .map_err(err("open file"))?;
        init.Initialize(&stream, STGM_READ.0).map_err(err("Initialize"))?;
        let provider: IThumbnailProvider = init.cast().map_err(err("IThumbnailProvider"))?;
        let mut hbmp = HBITMAP::default();
        let mut alpha = WTS_ALPHATYPE::default();
        provider.GetThumbnail(size, &mut hbmp, &mut alpha).map_err(err("GetThumbnail"))?;
        // The bitmap is ours; the DLL can go (DllCanUnloadNow says no, but the
        // objects are released by now, so it's safe here).
        drop((provider, init, factory));
        let _ = FreeLibrary(module);
        Ok(hbmp)
    }
}

fn shell(file: &str, size: u32) -> Result<HBITMAP, String> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().map_err(err("CoInitializeEx"))?;
        let path = std::fs::canonicalize(file).map_err(|e| format!("{file}: {e}"))?;
        // canonicalize gives a \\?\ path; the shell wants a plain one.
        let path = path.to_string_lossy().trim_start_matches(r"\\?\").to_string();
        let item: IShellItemImageFactory =
            SHCreateItemFromParsingName(&HSTRING::from(path.as_str()), None)
                .map_err(err("SHCreateItemFromParsingName"))?;
        let s = SIZE { cx: size as i32, cy: size as i32 };
        item.GetImage(s, SIIGBF_THUMBNAILONLY).map_err(err("GetImage (thumbnail only)"))
    }
}

/// Write the bitmap as a 32-bit .bmp and sanity-check it.
fn save(hbmp: HBITMAP, size: u32, out: &str) -> Result<String, String> {
    unsafe {
        let mut bm = BITMAP::default();
        if GetObjectW(HGDIOBJ(hbmp.0), std::mem::size_of::<BITMAP>() as i32, Some((&mut bm as *mut BITMAP).cast())) == 0 {
            return Err("GetObject failed".into());
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        let dc = CreateCompatibleDC(None);
        let lines = GetDIBits(dc, hbmp, 0, h as u32, Some(px.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
        let _ = DeleteDC(dc);
        let _ = DeleteObject(HGDIOBJ(hbmp.0));
        if lines != h {
            return Err(format!("GetDIBits returned {lines} of {h} rows"));
        }

        // BMP: 14-byte file header + 40-byte info header, top-down rows.
        let mut bmp = Vec::with_capacity(54 + px.len());
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&(54 + px.len() as u32).to_le_bytes());
        bmp.extend_from_slice(&[0; 4]);
        bmp.extend_from_slice(&54u32.to_le_bytes());
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&w.to_le_bytes());
        bmp.extend_from_slice(&(-h).to_le_bytes());
        bmp.extend_from_slice(&1u16.to_le_bytes());
        bmp.extend_from_slice(&32u16.to_le_bytes());
        bmp.extend_from_slice(&[0; 24]);
        bmp.extend_from_slice(&px);
        std::fs::write(out, &bmp).map_err(|e| format!("{out}: {e}"))?;

        let colours: HashSet<&[u8]> = px.chunks_exact(4).map(|p| &p[..3]).collect();
        let big = size as i32;
        if w > big || h > big || (w != big && h != big) {
            return Err(format!("{w}x{h} doesn't fit {size}x{size} snugly"));
        }
        if colours.len() < 20 {
            return Err(format!("{w}x{h} has only {} colours", colours.len()));
        }
        Ok(format!("{w}x{h}, {} colours, written to {out}", colours.len()))
    }
}
