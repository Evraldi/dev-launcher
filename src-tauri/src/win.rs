// Interop Windows minimal tanpa menambah dependency:
// - named mutex untuk single-instance
// - MessageBoxW untuk notifikasi saat instance kedua dibuka
// - ShellExecuteW untuk membuka URL tanpa lewat shell (anti command injection)
#![cfg(windows)]

use std::ffi::c_void;
use std::sync::Mutex;

type Handle = isize;
type Bool = i32;

const ERROR_ALREADY_EXISTS: u32 = 183;
const MB_OK: u32 = 0x0000_0000;
const MB_ICONINFORMATION: u32 = 0x0000_0040;
const SW_SHOWNORMAL: i32 = 1;

#[link(name = "kernel32")]
extern "system" {
    fn CreateMutexW(attributes: *mut c_void, initial_owner: Bool, name: *const u16) -> Handle;
    fn GetLastError() -> u32;
    fn SetLastError(code: u32);
    fn CloseHandle(handle: Handle) -> Bool;
}

#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(hwnd: Handle, text: *const u16, caption: *const u16, flags: u32) -> i32;
}

#[link(name = "shell32")]
extern "system" {
    fn ShellExecuteW(
        hwnd: Handle,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
    ) -> Handle;
}

/// Handle mutex disimpan agar tetap hidup selama proses berjalan.
static INSTANCE_HANDLE: Mutex<Handle> = Mutex::new(0);

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Ambil lock single-instance. `false` berarti instance lain sudah jalan.
pub fn acquire_single_instance() -> bool {
    let name = wide("Local\\dev-launcher-single-instance");
    // GetLastError hanya bermakna kalau error sebelumnya direset.
    unsafe { SetLastError(0) };
    let handle = unsafe { CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr()) };
    if handle == 0 {
        // Gagal membuat mutex: jangan blokir aplikasi.
        return true;
    }
    let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if already {
        unsafe {
            CloseHandle(handle);
        }
        false
    } else {
        *INSTANCE_HANDLE.lock().unwrap_or_else(|p| p.into_inner()) = handle;
        true
    }
}

pub fn message_box(text: &str, caption: &str) {
    let text = wide(text);
    let caption = wide(caption);
    unsafe {
        MessageBoxW(
            0,
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

/// Buka URL lewat ShellExecuteW (tidak mem-parsing `&`, `|`, `%`, dsb.).
pub fn open_url(url: &str) -> Result<(), String> {
    let operation = wide("open");
    let file = wide(url);
    let result = unsafe {
        ShellExecuteW(
            0,
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if result > 32 {
        Ok(())
    } else {
        Err(format!("Gagal membuka URL (kode {result})"))
    }
}
