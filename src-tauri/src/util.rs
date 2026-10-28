// Konstanta & helper lintas-platform.
// Pada Windows kita mencegah child process memunculkan jendela console.

#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Timestamp lokal untuk log: `YYYY-MM-DD HH:MM:SS`.
pub fn timestamp_now() -> String {
    use chrono::Local;
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Buang escape sequence ANSI (warna/kursor) dari output child process.
/// Tanpa ini, log panel menampilkan kode mentah seperti `[32m[1mVITE[22m`.
pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            // CSI: ESC [ ... <final byte 0x40..=0x7E>
            Some('[') => {
                chars.next();
                for cc in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&cc) {
                        break;
                    }
                }
            }
            // OSC: ESC ] ... terminated by BEL or ST (ESC \)
            Some(']') => {
                chars.next();
                while let Some(cc) = chars.next() {
                    if cc == '\u{07}' {
                        break;
                    }
                    if cc == '\u{1b}' {
                        if chars.peek() == Some(&'\\') {
                            chars.next();
                        }
                        break;
                    }
                }
            }
            // Escape dua/beberapa karakter lain: ESC + satu karakter akhir.
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::strip_ansi;

    #[test]
    fn strips_sgr_colors() {
        assert_eq!(
            strip_ansi("\u{1b}[32m\u{1b}[1mVITE\u{1b}[22m v8.0.11 ready"),
            "VITE v8.0.11 ready"
        );
    }

    #[test]
    fn strips_cursor_and_osc() {
        assert_eq!(strip_ansi("a\u{1b}[2Kb\u{1b}]0;title\u{07}c"), "abc");
    }

    #[test]
    fn keeps_plain_text() {
        assert_eq!(strip_ansi("npm run dev"), "npm run dev");
    }
}
