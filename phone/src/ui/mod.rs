//! Dumbphone UI: a screen stack driven by keypad events, rendered into a
//! fixed 24x10 character frame with status bar, title and soft-key labels.
use crate::at::{Clock, Link};
use crate::prelude::*;
use crate::services::{Core, Event, Transport};
use crate::store::Storage;
use crate::util::{self, center, char_len, pad};

pub mod font;
pub mod raster;
pub mod screens;
pub mod t9;

pub const W: usize = 24;
pub const ROWS: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// keypad 0-9, * and #
    Num(char),
    /// other printable character typed on a keyboard (text entry shortcut)
    Char(char),
    Up,
    Down,
    Left,
    Right,
    /// centre key / select
    Ok,
    /// clear / back
    Back,
    /// green key
    Call,
    /// red key
    End,
    /// left soft key (options)
    Opt,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Frame {
    pub status: String,
    pub title: String,
    pub lines: Vec<String>,
    pub sel: Option<usize>,
    /// [left soft key (Opt), centre (OK), right (Back)]
    pub soft: [String; 3],
}

impl Frame {
    pub fn line(&mut self, s: &str) {
        self.lines.push(s.to_string());
    }
    pub fn soft(&mut self, l: &str, c: &str, r: &str) {
        self.soft = [l.to_string(), c.to_string(), r.to_string()];
    }
}

pub enum Nav {
    Stay,
    Push(Box<dyn Screen>),
    Replace(Box<dyn Screen>),
    Pop,
    Home,
    /// Close a chooser and hand (index, tag) to the screen below.
    Result(usize, u32),
}

pub trait Screen {
    fn render(&self, c: &Core, f: &mut Frame);
    fn key(&mut self, c: &mut Core, k: Key) -> Nav;
    fn tick(&mut self, _c: &mut Core) -> Nav {
        Nav::Stay
    }
    fn result(&mut self, _c: &mut Core, _index: usize, _tag: u32) -> Nav {
        Nav::Stay
    }
    fn is_call(&self) -> bool {
        false
    }
    fn is_alarm(&self) -> bool {
        false
    }
    fn thread(&self) -> Option<&str> {
        None
    }
}

pub struct Phone {
    pub core: Core,
    stack: Vec<Box<dyn Screen>>,
    toast: Option<(String, u64)>,
    storage: Option<Box<dyn Storage>>,
    pub sound: crate::audio::Audio,
    saved: Vec<u8>,
    last_save: u64,
    pub restored: bool,
    save_warned: bool,
}

impl Phone {
    pub fn new(link: Box<dyn Link>, clock: Clock) -> Phone {
        Phone { core: Core::new(link, clock), stack: Vec::new(), toast: None, sound: crate::audio::Audio::new(), storage: None, saved: Vec::new(), last_save: 0, restored: false, save_warned: false }
    }

    /// A restart or power-off the user asked for. Data is saved first;
    /// returns None (and tells the user) if saving failed.
    pub fn take_power_request(&mut self) -> Option<crate::services::Power> {
        let p = self.core.power_request.take()?;
        if self.storage.is_some() && !self.save() {
            self.toast("Couldn't save data: not restarting");
            return None;
        }
        Some(p)
    }

    /// Pull 48 kHz mono samples for the speaker.
    pub fn audio(&mut self, out: &mut [i16]) {
        self.sound.fill(&mut self.core, out);
    }

    /// Attach persistent storage; call before `boot`.
    pub fn set_storage(&mut self, s: Box<dyn Storage>) {
        self.storage = Some(s);
    }

    /// Write the user's data now if it changed since the last save.
    pub fn save(&mut self) -> bool {
        let snap = self.core.snapshot();
        if snap == self.saved {
            return true;
        }
        let ok = self.storage.as_mut().is_some_and(|s| s.save(&snap));
        if ok {
            self.saved = snap;
        }
        ok
    }

    pub fn boot(&mut self) {
        if let Some(data) = self.storage.as_mut().and_then(|s| s.load()) {
            self.restored = self.core.restore(&data);
            if self.restored {
                self.saved = data;
            }
        }
        self.core.boot();
        self.stack.clear();
        self.stack.push(Box::new(screens::Home));
        if self.core.sim_locked {
            self.stack.push(Box::new(screens::PinEntry::default()));
        }
    }

    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    fn toast(&mut self, s: &str) {
        self.toast = Some((s.to_string(), self.core.now + 2500));
    }

    fn apply(&mut self, nav: Nav) {
        match nav {
            Nav::Stay => {}
            Nav::Push(s) => self.stack.push(s),
            Nav::Replace(s) => {
                if self.stack.len() > 1 {
                    self.stack.pop();
                }
                self.stack.push(s);
            }
            Nav::Pop => {
                if self.stack.len() > 1 {
                    self.stack.pop();
                }
            }
            Nav::Home => self.stack.truncate(1),
            Nav::Result(i, tag) => {
                if self.stack.len() > 1 {
                    self.stack.pop();
                }
                self.with_top(|s, c| s.result(c, i, tag));
            }
        }
    }

    fn with_top(&mut self, f: impl FnOnce(&mut dyn Screen, &mut Core) -> Nav) {
        if let Some(mut top) = self.stack.pop() {
            let nav = f(top.as_mut(), &mut self.core);
            self.stack.push(top);
            self.apply(nav);
        }
    }

    pub fn key(&mut self, k: Key) {
        // Notes are dismissed by any key press, which is still delivered.
        self.toast = None;
        if k == Key::End {
            if self.core.call.is_some() {
                self.core.hangup();
                self.drain_events();
            } else if !self.core.sim_locked {
                self.stack.truncate(1);
            }
            return;
        }
        if let Key::Num(d) = k {
            self.sound.cue(crate::audio::Cue::Key(d));
        }
        self.with_top(|s, c| s.key(c, k));
        self.drain_events();
    }

    pub fn step(&mut self, now: u64) {
        self.core.step(now);
        self.drain_events();
        self.with_top(|s, c| s.tick(c));
        if self.core.alarm_ringing && !self.stack.iter().any(|s| s.is_alarm()) {
            self.core.alarm_ringing = false;
        }
        if self.storage.is_some() && now.saturating_sub(self.last_save) >= 2000 {
            self.last_save = now;
            if self.save() {
                self.save_warned = false;
            } else if !self.save_warned {
                self.save_warned = true;
                self.toast("Storage full: delete old conversations");
            }
        }
        if self.toast.as_ref().is_some_and(|(_, t)| now > *t) {
            self.toast = None;
        }
    }

    fn drain_events(&mut self) {
        while let Some(e) = self.core.events.pop_front() {
            match e {
                Event::IncomingCall(_) => {
                    if !self.stack.iter().any(|s| s.is_call()) {
                        self.stack.push(Box::new(screens::CallScreen::default()));
                    }
                }
                Event::CallEnded { number, secs, reason } => {
                    self.stack.retain(|s| !s.is_call());
                    if self.stack.is_empty() {
                        self.stack.push(Box::new(screens::Home));
                    }
                    let who = self.core.name_for(&number);
                    self.sound.cue(if reason == "Busy" { crate::audio::Cue::Busy } else { crate::audio::Cue::CallEnd });
                    let msg = if secs > 0 { format!("{} {}", reason, util::duration(secs)) } else { format!("{}: {}", reason, who) };
                    self.toast(&msg);
                }
                Event::NewMessage { number, transport } => {
                    let viewing = self.stack.last().and_then(|s| s.thread()).is_some_and(|t| util::same_number(t, &number));
                    if !self.stack.last().is_some_and(|s| s.is_call()) {
                        self.sound.cue(crate::audio::Cue::Message);
                    }
                    if !viewing && !self.stack.last().is_some_and(|s| s.is_call()) {
                        let kind = if transport == Transport::Mms { "MMS" } else { "Message" };
                        let who = self.core.name_for(&number);
                        self.toast(&format!("{} from {}", kind, who));
                    }
                }
                Event::Ussd(t) => self.stack.push(Box::new(screens::MsgBox::new("Network", &t))),
                Event::Alert { title, text } => self.stack.push(Box::new(screens::MsgBox::alert(&title, &text))),
                Event::Alarm => {
                    self.core.alarm_ringing = true;
                    self.stack.push(Box::new(screens::AlarmRing));
                }
                Event::Toast(t) => self.toast(&t),
            }
        }
    }

    pub fn render(&self) -> Frame {
        let c = &self.core;
        let mut f = Frame::default();
        f.status = status_bar(c);
        if let Some(top) = self.stack.last() {
            top.render(c, &mut f);
        }
        f.lines.truncate(ROWS);
        while f.lines.len() < ROWS {
            f.lines.push(String::new());
        }
        if let Some((t, _)) = &self.toast {
            let wrapped = util::wrap(t, W - 4);
            let n = wrapped.len().min(3);
            let top = (ROWS - n - 2) / 2;
            f.lines[top] = format!("┌{}┐", "─".repeat(W - 2));
            for (i, l) in wrapped.iter().take(n).enumerate() {
                f.lines[top + 1 + i] = format!("│{}│", center(l, W - 2));
            }
            f.lines[top + 1 + n] = format!("└{}┘", "─".repeat(W - 2));
            f.sel = None;
        }
        f
    }

    /// Render the frame as an ANSI terminal "phone".
    pub fn render_ansi(&self) -> String {
        ansi(&self.render())
    }
}

fn status_bar(c: &Core) -> String {
    let n = &c.net;
    let mut left = String::new();
    if c.settings.airplane {
        left.push_str("✈ ");
    } else {
        left.push_str(n.rat());
        left.push_str(["    ", "▂   ", "▂▄  ", "▂▄▆ ", "▂▄▆█"][n.bars() as usize]);
    }
    if c.rcs_usable() {
        left.push_str(" Chat");
    }
    if c.unread_total() > 0 {
        left.push_str(" ✉");
    }
    if c.missed > 0 {
        left.push_str(" ☎");
    }
    if c.settings.alarm_on {
        left.push_str(" ◷");
    }
    let right = format!("{}% {}", n.battery, util::hhmm(c.local()));
    let space = W.saturating_sub(char_len(&left) + char_len(&right));
    let mut s = left;
    for _ in 0..space {
        s.push(' ');
    }
    s.push_str(&right);
    pad(&s, W)
}

pub fn ansi(f: &Frame) -> String {
    let bar = "─".repeat(W);
    let mut s = String::from("\x1b[H");
    s.push_str(&format!("┌{}┐\x1b[K\r\n", bar));
    s.push_str(&format!("│\x1b[7m{}\x1b[0m│\x1b[K\r\n", pad(&f.status, W)));
    s.push_str(&format!("│\x1b[1m{}\x1b[0m│\x1b[K\r\n", center(&f.title, W)));
    s.push_str(&format!("├{}┤\x1b[K\r\n", bar));
    for (i, l) in f.lines.iter().enumerate() {
        if f.sel == Some(i) {
            s.push_str(&format!("│\x1b[7m{}\x1b[0m│\x1b[K\r\n", pad(l, W)));
        } else {
            s.push_str(&format!("│{}│\x1b[K\r\n", pad(l, W)));
        }
    }
    s.push_str(&format!("├{}┤\x1b[K\r\n", bar));
    let (l, m, r) = (&f.soft[0], &f.soft[1], &f.soft[2]);
    let lw = W / 3;
    let soft = format!("{}{}{}", pad(l, lw), center(m, W - 2 * lw), right_align(r, lw));
    s.push_str(&format!("│\x1b[1m{}\x1b[0m│\x1b[K\r\n", soft));
    s.push_str(&format!("└{}┘\x1b[K\r\n", bar));
    s.push_str("\x1b[K\r\n");
    s.push_str(" [ ] soft key   Enter OK\x1b[K\r\n");
    s.push_str(" Bksp back  Tab call  ` end\x1b[K\r\n");
    s.push_str(" 0-9 * # keypad  arrows nav\x1b[K\r\n");
    s.push_str("\x1b[J");
    s
}

fn right_align(s: &str, n: usize) -> String {
    let s = util::trunc(s, n);
    let mut out = String::new();
    for _ in char_len(&s)..n {
        out.push(' ');
    }
    out.push_str(&s);
    out
}

/// Visible window of a list so that `sel` stays on screen.
pub fn list(f: &mut Frame, items: &[String], sel: usize, rows: usize) {
    let top = sel.saturating_sub(rows.saturating_sub(1));
    let start = f.lines.len();
    for it in items.iter().skip(top).take(rows) {
        f.lines.push(it.clone());
    }
    if !items.is_empty() {
        f.sel = Some(start + sel - top);
    }
}

pub fn nav_list(k: Key, sel: &mut usize, len: usize) -> bool {
    if len == 0 {
        return false;
    }
    match k {
        Key::Up => *sel = if *sel == 0 { len - 1 } else { *sel - 1 },
        Key::Down => *sel = (*sel + 1) % len,
        Key::Num(d @ '1'..='9') if (d as usize - '1' as usize) < len => *sel = d as usize - '1' as usize,
        _ => return false,
    }
    true
}

#[cfg(test)]
mod t;
