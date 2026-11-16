//! Brand faces from the CatWM design system: Alpino for headings and
//! Pilcrow Rounded for the wordmark. They ship next to the gresource bundle
//! and are registered for this process only, before GTK builds its font map.

use std::path::Path;

use tracing::warn;

use crate::PKGDATADIR;

const FONTS: &[&str] = &[
    "Alpino-Bold.otf",
    "PilcrowRounded-Bold.otf",
    "PilcrowRounded-Semibold.otf",
];

pub fn register() {
    let dir = Path::new(PKGDATADIR).join("fonts");
    for name in FONTS {
        let path = dir.join(name);
        if !register_file(&path) {
            // The stylesheet falls back to the system font, so this is cosmetic.
            warn!("Could not register font {}", path.display());
        }
    }
}

#[cfg(target_os = "macos")]
fn register_file(path: &Path) -> bool {
    use std::{
        ffi::c_void,
        os::unix::ffi::OsStrExt,
        ptr,
    };

    const PROCESS_SCOPE: u32 = 1;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: *const c_void, buffer: *const u8, length: isize, is_directory: u8,
        ) -> *const c_void;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "CoreText", kind = "framework")]
    unsafe extern "C" {
        fn CTFontManagerRegisterFontsForURL(
            url: *const c_void, scope: u32, error: *mut *const c_void,
        ) -> u8;
    }

    let bytes = path.as_os_str().as_bytes();
    unsafe {
        let url = CFURLCreateFromFileSystemRepresentation(
            ptr::null(),
            bytes.as_ptr(),
            bytes.len() as isize,
            0,
        );
        if url.is_null() {
            return false;
        }
        let ok = CTFontManagerRegisterFontsForURL(url, PROCESS_SCOPE, ptr::null_mut()) != 0;
        CFRelease(url);
        ok
    }
}

#[cfg(not(target_os = "macos"))]
fn register_file(path: &Path) -> bool {
    use std::{
        ffi::{
            CString,
            c_char,
            c_int,
            c_void,
        },
        os::unix::ffi::OsStrExt,
        ptr,
    };

    #[link(name = "fontconfig")]
    unsafe extern "C" {
        fn FcConfigAppFontAddFile(config: *mut c_void, file: *const c_char) -> c_int;
    }

    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    unsafe { FcConfigAppFontAddFile(ptr::null_mut(), path.as_ptr()) != 0 }
}
