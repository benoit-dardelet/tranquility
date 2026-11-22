//! Terminal byte stream → phone keypad events.
use phone::ui::Key;

#[derive(Default)]
pub struct Decoder {
    st: u8,
    esc_at: u64,
    utf8: heapless_utf8::Buf,
}

mod heapless_utf8 {
    #[derive(Default)]
    pub struct Buf {
        pub b: [u8; 4],
        pub n: usize,
        pub want: usize,
    }
}

impl Decoder {
    pub fn feed(&mut self, b: u8, now: u64) -> Option<Key> {
        match self.st {
            1 => {
                if b == b'[' || b == b'O' {
                    self.st = 2;
                    return None;
                }
                self.st = 0;
                return Some(Key::Exit);
            }
            2 => {
                self.st = if b.is_ascii_digit() { 3 } else { 0 };
                return match b {
                    b'A' => Some(Key::Up),
                    b'B' => Some(Key::Down),
                    b'C' => Some(Key::Right),
                    b'D' => Some(Key::Left),
                    _ => None,
                };
            }
            3 => {
                if b == b'~' || !b.is_ascii_digit() {
                    self.st = 0;
                }
                return None;
            }
            _ => {}
        }
        if self.utf8.want > 0 {
            self.utf8.b[self.utf8.n] = b;
            self.utf8.n += 1;
            if self.utf8.n == self.utf8.want {
                self.utf8.want = 0;
                let s = core::str::from_utf8(&self.utf8.b[..self.utf8.n]).ok()?;
                return s.chars().next().map(Key::Char);
            }
            return None;
        }
        match b {
            0x1B => {
                self.st = 1;
                self.esc_at = now;
                None
            }
            b'\r' | b'\n' => Some(Key::Ok),
            0x7F | 0x08 => Some(Key::Back),
            b'\t' => Some(Key::Call),
            b'`' => Some(Key::End),
            b'[' | b']' => Some(Key::Opt),
            b'0'..=b'9' | b'*' | b'#' => Some(Key::Num(b as char)),
            0x20..=0x7E => Some(Key::Char(b as char)),
            0xC0..=0xDF | 0xE0..=0xEF | 0xF0..=0xF7 => {
                self.utf8.b[0] = b;
                self.utf8.n = 1;
                self.utf8.want = if b >= 0xF0 { 4 } else if b >= 0xE0 { 3 } else { 2 };
                None
            }
            _ => None,
        }
    }

    /// A lone ESC (no sequence following) leaves the current screen.
    pub fn flush(&mut self, now: u64) -> Option<Key> {
        if self.st == 1 && now.saturating_sub(self.esc_at) > 60 {
            self.st = 0;
            return Some(Key::Exit);
        }
        None
    }
}
