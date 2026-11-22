//! virtio-input keyboard → phone keypad events (QEMU graphical window).
use crate::virtio::{dma, Mmio, Queue};
use alloc::vec::Vec;
use phone::ui::Key;

const N: usize = 32;

pub struct VirtioInput {
    m: Mmio,
    q: Queue,
    bufs: Vec<*mut u8>,
    heads: Vec<(u16, usize)>,
    shift: bool,
}

impl VirtioInput {
    pub fn new(m: Mmio) -> Option<VirtioInput> {
        if !m.begin() {
            return None;
        }
        let q = m.queue(0, N as u16)?;
        let _status = m.queue(1, 4);
        m.finish();
        let mut v = VirtioInput { m, q, bufs: (0..N).map(|_| dma(8)).collect(), heads: Vec::new(), shift: false };
        for i in 0..N {
            v.post(i);
        }
        v.m.notify(&v.q);
        Some(v)
    }

    fn post(&mut self, i: usize) {
        if let Some(h) = self.q.submit(&[(self.bufs[i] as u64, 8, true)]) {
            self.heads.push((h, i));
        }
    }

    pub fn poll(&mut self, out: &mut Vec<Key>) {
        let mut any = false;
        while let Some((head, _)) = self.q.pop() {
            let i = match self.heads.iter().position(|&(h, _)| h == head) {
                Some(p) => self.heads.swap_remove(p).1,
                None => continue,
            };
            let b = self.bufs[i];
            let (ty, code, val) = unsafe { ((b as *const u16).read_volatile(), (b.add(2) as *const u16).read_volatile(), (b.add(4) as *const u32).read_volatile()) };
            if ty == 1 {
                if code == 42 || code == 54 {
                    self.shift = val != 0;
                } else if val != 0 {
                    if let Some(k) = map(code, self.shift) {
                        out.push(k);
                    }
                }
            }
            self.post(i);
            any = true;
        }
        if any {
            self.m.notify(&self.q);
        }
    }
}

/// Linux input keycodes → keypad.
fn map(code: u16, shift: bool) -> Option<Key> {
    const DIGITS: [(u16, char); 10] = [(2, '1'), (3, '2'), (4, '3'), (5, '4'), (6, '5'), (7, '6'), (8, '7'), (9, '8'), (10, '9'), (11, '0')];
    const LETTERS: &[(u16, char)] = &[
        (16, 'q'), (17, 'w'), (18, 'e'), (19, 'r'), (20, 't'), (21, 'y'), (22, 'u'), (23, 'i'), (24, 'o'), (25, 'p'),
        (30, 'a'), (31, 's'), (32, 'd'), (33, 'f'), (34, 'g'), (35, 'h'), (36, 'j'), (37, 'k'), (38, 'l'),
        (44, 'z'), (45, 'x'), (46, 'c'), (47, 'v'), (48, 'b'), (49, 'n'), (50, 'm'),
    ];
    Some(match (code, shift) {
        (4, true) => Key::Num('#'),
        (9, true) => Key::Num('*'),
        (13, true) => Key::Char('+'),
        (2, true) => Key::Char('!'),
        (53, true) => Key::Char('?'),
        (55, _) => Key::Num('*'),
        (28, _) | (96, _) => Key::Ok,
        (14, _) => Key::Back,
        (15, _) => Key::Call,
        (1, _) => Key::Exit,
        (41, _) | (107, _) => Key::End, // ` and the End key
        (26, _) | (27, _) => Key::Opt,
        (103, _) => Key::Up,
        (108, _) => Key::Down,
        (105, _) => Key::Left,
        (106, _) => Key::Right,
        (57, _) => Key::Char(' '),
        (52, _) => Key::Char('.'),
        (51, _) => Key::Char(','),
        (12, _) => Key::Char('-'),
        (53, _) => Key::Char('/'),
        (40, _) => Key::Char('\''),
        (c, _) => {
            if let Some(&(_, d)) = DIGITS.iter().find(|(k, _)| *k == c) {
                Key::Num(d)
            } else if let Some(&(_, l)) = LETTERS.iter().find(|(k, _)| *k == c) {
                Key::Char(if shift { l.to_ascii_uppercase() } else { l })
            } else {
                return None;
            }
        }
    })
}
