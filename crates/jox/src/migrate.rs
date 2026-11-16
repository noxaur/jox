//! One-time moves from the tsukimi era.
//!
//! The GSettings path is still `/moe/tsukimi/`, so dconf (Linux) keeps working
//! as is. On macOS GLib stores settings in the NSUserDefaults domain of the app:
//! the bundle ID inside Jox.app, otherwise the executable name, which was
//! `tsukimi` and is now `jox`. The newest older domain is copied over once,
//! before GSettings is first read.

#[cfg(target_os = "macos")]
pub fn run() {
    macos::copy_older_defaults();
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

    /// Domains that may hold settings from before, newest first.
    const OLDER: [&str; 2] = ["jox", "tsukimi"];

    pub fn copy_older_defaults() {
        unsafe {
            let accounts = cfstr("/moe/tsukimi/accounts");
            let existing = CFPreferencesCopyAppValue(accounts, kCFPreferencesCurrentApplication);
            CFRelease(accounts);
            if !existing.is_null() {
                CFRelease(existing);
                return;
            }

            for name in OLDER {
                if copy_domain(name) {
                    tracing::info!("Moved settings from {name}");
                    return;
                }
            }
        }
    }

    /// Copies a domain that has accounts into the current one.
    unsafe fn copy_domain(name: &str) -> bool {
        unsafe {
            let old = cfstr(name);
            let accounts = cfstr("/moe/tsukimi/accounts");
            let found = CFPreferencesCopyAppValue(accounts, old);
            CFRelease(accounts);
            if found.is_null() {
                CFRelease(old);
                return false;
            }
            CFRelease(found);

            let mut copied = false;
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
                    copied = true;
                }
                CFRelease(keys);
            }
            CFRelease(old);
            copied
        }
    }
}
