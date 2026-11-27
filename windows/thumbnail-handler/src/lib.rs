//! Windows Explorer thumbnail handler for `.cptv` files.
//!
//! A COM in-process server: Explorer (in a separate, isolated host process)
//! creates [`ThumbnailProvider`] through `DllGetClassObject`, hands it the file
//! as an `IStream` (`IInitializeWithStream`), then asks for a bitmap
//! (`IThumbnailProvider::GetThumbnail`). The picture is drawn by the shared
//! core (`cptv_native_viewers::thumbnail`), the same frame the Linux and macOS
//! thumbnails show.
//!
//! Registration (done by the installer, packaging/windows/*.iss):
//!   HKCR\CLSID\{FBF13CFA-0D80-4899-926E-C50711CDFA5A}\InprocServer32 = <this DLL>,
//!       ThreadingModel = Apartment
//!   HKCR\.cptv\ShellEx\{e357fccd-a995-4576-b01f-234630154e96} = {FBF13CFA-…}

#![allow(non_snake_case)]

use std::cell::RefCell;
use std::ffi::c_void;
use std::io::{self, Read};
use std::panic::{catch_unwind, AssertUnwindSafe};

use cptv_native_viewers::palette::Palette;
use cptv_native_viewers::source::{Opener, Reader};
use cptv_native_viewers::thumbnail;
use windows::core::{implement, Interface, Ref, BOOL, GUID, HRESULT};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_POINTER, E_UNEXPECTED, S_FALSE,
};
use windows::Win32::Graphics::Gdi::{
    CreateDIBSection, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::System::Com::{
    IClassFactory, IClassFactory_Impl, IStream, STREAM_SEEK_SET,
};
use windows::Win32::UI::Shell::PropertiesSystem::{
    IInitializeWithStream, IInitializeWithStream_Impl,
};
use windows::Win32::UI::Shell::{
    IThumbnailProvider, IThumbnailProvider_Impl, WTSAT_RGB, WTS_ALPHATYPE,
};

/// CLSID of the thumbnail handler (must match the installer's registry entries).
pub const CLSID_CPTV_THUMBNAIL: GUID = GUID::from_u128(0xFBF13CFA_0D80_4899_926E_C50711CDFA5A);

/// Largest thumbnail we'll draw; Explorer asks for at most 2560 px.
const MAX_SIZE: u32 = 2560;

// --- Reading the file through Explorer's IStream ------------------------------

/// `Read` over a COM stream. Explorer's streams are seekable, which is what
/// lets the core rewind (re-open) the clip.
struct StreamReader(IStream);

impl Read for StreamReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut n = 0u32;
        let len = buf.len().min(u32::MAX as usize) as u32;
        // S_FALSE (fewer bytes than asked, i.e. end of stream) is a success.
        unsafe { self.0.Read(buf.as_mut_ptr().cast(), len, Some(&mut n)) }
            .ok()
            .map_err(io::Error::other)?;
        Ok(n as usize)
    }
}

fn stream_opener(stream: IStream) -> Opener {
    Box::new(move || {
        unsafe { stream.Seek(0, STREAM_SEEK_SET, None) }.map_err(io::Error::other)?;
        Ok(Box::new(io::BufReader::with_capacity(64 * 1024, StreamReader(stream.clone()))) as Reader)
    })
}

// --- The thumbnail provider ---------------------------------------------------

#[implement(IInitializeWithStream, IThumbnailProvider)]
pub struct ThumbnailProvider {
    stream: RefCell<Option<IStream>>,
}

impl IInitializeWithStream_Impl for ThumbnailProvider_Impl {
    fn Initialize(&self, pstream: Ref<IStream>, _grfmode: u32) -> windows::core::Result<()> {
        let stream = pstream.ok()?.clone();
        let mut slot = self.stream.borrow_mut();
        if slot.is_some() {
            // The handler is initialised once per file.
            return Err(E_UNEXPECTED.into());
        }
        *slot = Some(stream);
        Ok(())
    }
}

impl IThumbnailProvider_Impl for ThumbnailProvider_Impl {
    fn GetThumbnail(
        &self,
        cx: u32,
        phbmp: *mut HBITMAP,
        pdwalpha: *mut WTS_ALPHATYPE,
    ) -> windows::core::Result<()> {
        if phbmp.is_null() || pdwalpha.is_null() {
            return Err(E_POINTER.into());
        }
        let stream = self.stream.borrow().clone().ok_or(windows::core::Error::from(E_UNEXPECTED))?;
        // Never let a panic unwind into Explorer's host process.
        let hbmp = catch_unwind(AssertUnwindSafe(|| render_bitmap(stream, cx)))
            .map_err(|_| windows::core::Error::from(E_FAIL))??;
        unsafe {
            *phbmp = hbmp;
            *pdwalpha = WTSAT_RGB;
        }
        Ok(())
    }
}

/// Draw the thumbnail into a new top-down 32-bit DIB section.
fn render_bitmap(stream: IStream, cx: u32) -> windows::core::Result<HBITMAP> {
    let size = cx.clamp(1, MAX_SIZE) as usize;
    // A plain HRESULT: Explorer never shows the message, and attaching one
    // calls RoOriginateErrorW, which Wine doesn't implement.
    let thumb = thumbnail::render(stream_opener(stream), size, Palette::default())
        .map_err(|_| windows::core::Error::from(E_FAIL))?;
    let (w, h) = (thumb.width, thumb.height);

    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32), // negative: top row first
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    let hbmp = unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0) }?;
    if bits.is_null() {
        return Err(E_FAIL.into());
    }
    // 32-bit DIB pixels are B, G, R, X bytes.
    let dst = unsafe { std::slice::from_raw_parts_mut(bits.cast::<u8>(), w * h * 4) };
    for (px, rgb) in dst.chunks_exact_mut(4).zip(thumb.rgb.chunks_exact(3)) {
        px.copy_from_slice(&[rgb[2], rgb[1], rgb[0], 0xff]);
    }
    Ok(hbmp)
}

// --- Class factory and DLL exports -------------------------------------------

#[implement(IClassFactory)]
struct ClassFactory;

impl IClassFactory_Impl for ClassFactory_Impl {
    fn CreateInstance(
        &self,
        punkouter: Ref<windows::core::IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut c_void,
    ) -> windows::core::Result<()> {
        if ppvobject.is_null() || riid.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *ppvobject = std::ptr::null_mut() };
        if !punkouter.is_null() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let provider: IInitializeWithStream =
            ThumbnailProvider { stream: RefCell::new(None) }.into();
        unsafe { provider.query(riid, ppvobject) }.ok()
    }

    fn LockServer(&self, _flock: BOOL) -> windows::core::Result<()> {
        Ok(())
    }
}

/// COM entry point: hands out the class factory for our CLSID.
#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if rclsid.is_null() || riid.is_null() || ppv.is_null() {
        return E_POINTER;
    }
    *ppv = std::ptr::null_mut();
    if *rclsid != CLSID_CPTV_THUMBNAIL {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = ClassFactory.into();
    factory.query(riid, ppv)
}

/// COM entry point. The handler runs in a short-lived isolated host process,
/// so just stay loaded rather than counting objects.
#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE
}
