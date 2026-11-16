//! One-time moves from the tsukimi era.
//!
//! The GSettings path is still `/moe/tsukimi/`, so dconf (Linux) keeps working
//! as is. On macOS GLib stores settings in the NSUserDefaults domain of the
//! executable, which was `tsukimi` and is now `jox`, so that domain is copied
//! over once, before GSettings is first read.

#[cfg(target_os = "macos")]
pub fn run() {
    macos::copy_tsukimi_defaults();
}

#[cfg(not(target_os = "macos"))]
pub fn run() {}

#[cfg(target_os = "macos")]
mod macos {
    use std::{
        ffi::{
            CString,
            c_char,
            c_void,
        },
        ptr,
    };

    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;

    const UTF8: u32 = 0x0800_0100;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFPreferencesCurrentApplication: CFStringRef;
        static kCFPreferencesCurrentUser: CFStringRef;
        static kCFPreferencesAnyHost: CFStringRef;

        fn CFStringCreateWithCString(
            alloc: *const c_void, cstr: *const c_char, encoding: u32,
        ) -> CFStringRef;
        fn CFRelease(cf: CFTypeRef);
        fn CFPreferencesCopyAppValue(key: CFStringRef, app: CFStringRef) -> CFTypeRef;
        fn CFPreferencesCopyKeyList(
            app: CFStringRef, user: CFStringRef, host: CFStringRef,
        ) -> CFTypeRef;
        fn CFPreferencesCopyMultiple(
            keys: CFTypeRef, app: CFStringRef, user: CFStringRef, host: CFStringRef,
        ) -> CFTypeRef;
        fn CFPreferencesSetMultiple(
            set: CFTypeRef, remove: CFTypeRef, app: CFStringRef, user: CFStringRef,
            host: CFStringRef,
        );
        fn CFPreferencesAppSynchronize(app: CFStringRef) -> u8;
    }

    fn cfstr(s: &str) -> CFStringRef {
        let s = CString::new(s).expect("no NUL in constant");
        unsafe { CFStringCreateWithCString(ptr::null(), s.as_ptr(), UTF8) }
    }

    pub fn copy_tsukimi_defaults() {
        unsafe {
            let accounts = cfstr("/moe/tsukimi/accounts");
            let existing = CFPreferencesCopyAppValue(accounts, kCFPreferencesCurrentApplication);
            CFRelease(accounts);
            if !existing.is_null() {
                CFRelease(existing);
                return;
            }

            let old = cfstr("tsukimi");
            let keys = CFPreferencesCopyKeyList(old, kCFPreferencesCurrentUser, kCFPreferencesAnyHost);
            if !keys.is_null() {
                let values = CFPreferencesCopyMultiple(
                    keys,
                    old,
                    kCFPreferencesCurrentUser,
                    kCFPreferencesAnyHost,
                );
                if !values.is_null() {
                    CFPreferencesSetMultiple(
                        values,
                        ptr::null(),
                        kCFPreferencesCurrentApplication,
                        kCFPreferencesCurrentUser,
                        kCFPreferencesAnyHost,
                    );
                    CFPreferencesAppSynchronize(kCFPreferencesCurrentApplication);
                    CFRelease(values);
                    tracing::info!("Moved settings from tsukimi");
                }
                CFRelease(keys);
            }
            CFRelease(old);
        }
    }
}
