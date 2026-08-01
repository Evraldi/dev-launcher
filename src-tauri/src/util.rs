// Konstanta & helper lintas-platform.
// Pada Windows kita mencegah child process memunculkan jendela console.

#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
