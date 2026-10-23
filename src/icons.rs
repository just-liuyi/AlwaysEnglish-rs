//! Extracts the icon of an executable as a Slint image.

use std::ffi::c_void;

use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits, GetObjectW,
    HBITMAP, HDC, ReleaseDC,
};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::UI::Shell::{
    SHDefExtractIconW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};
use windows::core::HSTRING;

/// Icon of `path` at roughly `size` pixels, or `None` if the file has none / doesn't exist.
pub fn file_icon(path: &str, size: u32) -> Option<Image> {
    let wpath = HSTRING::from(path);
    unsafe {
        let mut hicon = HICON::default();
        let extracted = SHDefExtractIconW(&wpath, 0, 0, Some(&mut hicon), None, size).is_ok() && !hicon.is_invalid();
        if !extracted {
            // Falls back to the generic icon for the file type (also works for missing files).
            let mut info = SHFILEINFOW::default();
            let r = SHGetFileInfoW(
                &wpath,
                FILE_ATTRIBUTE_NORMAL,
                Some(&mut info),
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON | SHGFI_USEFILEATTRIBUTES,
            );
            if r == 0 || info.hIcon.is_invalid() {
                return None;
            }
            hicon = info.hIcon;
        }
        let image = hicon_to_image(hicon);
        let _ = DestroyIcon(hicon);
        image
    }
}

unsafe fn read_bitmap(hdc: HDC, bitmap: HBITMAP, width: i32, height: i32) -> Option<Vec<u8>> {
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let lines = unsafe {
        GetDIBits(
            hdc,
            bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr() as *mut c_void),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    (lines > 0).then_some(pixels)
}

unsafe fn hicon_to_image(hicon: HICON) -> Option<Image> {
    let mut icon_info = ICONINFO::default();
    unsafe { GetIconInfo(hicon, &mut icon_info).ok()? };
    let color = icon_info.hbmColor;
    let mask = icon_info.hbmMask;

    let result = (|| unsafe {
        if color.is_invalid() {
            return None;
        }
        let mut bm = BITMAP::default();
        if GetObjectW(color.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut c_void)) == 0 {
            return None;
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        if w <= 0 || h <= 0 {
            return None;
        }
        let hdc = GetDC(None);
        let mut pixels = read_bitmap(hdc, color, w, h);
        if let Some(px) = pixels.as_mut()
            && px.as_chunks::<4>().0.iter().all(|p| p[3] == 0)
        {
            // Icon without an alpha channel: derive transparency from the AND mask.
            if !mask.is_invalid()
                && let Some(mask_px) = read_bitmap(hdc, mask, w, h)
            {
                for (p, m) in px.as_chunks_mut::<4>().0.iter_mut().zip(mask_px.as_chunks::<4>().0) {
                    p[3] = if m[0] == 0 { 255 } else { 0 };
                }
            } else {
                px.as_chunks_mut::<4>().0.iter_mut().for_each(|p| p[3] = 255);
            }
        }
        ReleaseDC(None, hdc);
        let px = pixels?;
        let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(w as u32, h as u32);
        for (dst, src) in buffer.make_mut_slice().iter_mut().zip(px.as_chunks::<4>().0) {
            *dst = Rgba8Pixel { r: src[2], g: src[1], b: src[0], a: src[3] };
        }
        Some(Image::from_rgba8(buffer))
    })();

    unsafe {
        if !color.is_invalid() {
            let _ = DeleteObject(color.into());
        }
        if !mask.is_invalid() {
            let _ = DeleteObject(mask.into());
        }
    }
    result
}
