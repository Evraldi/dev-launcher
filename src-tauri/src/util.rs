// Konstanta & helper lintas-platform.
// Pada Windows kita mencegah child process memunculkan jendela console.

#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Timestamp lokal untuk log: `YYYY-MM-DD HH:MM:SS`.
pub fn timestamp_now() -> String {
    use chrono::Local;
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
