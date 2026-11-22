//! The phone's screens.
use super::t9::{Mode, TextInput};
use super::{list, nav_list, Frame, Key, Nav, Screen, ROWS, W};
use crate::bmp;
use crate::mms::Part;
use crate::prelude::*;
use crate::services::{CallState, Contact, Core, LogKind, Message, NetMode, Status, Transport};
use crate::sms;
use crate::util::{self, center, duration, hhmm, same_number, trunc, wrap};
use super::i18n::{self, Lang};

fn push<S: Screen + 'static>(s: S) -> Nav {
    Nav::Push(Box::new(s))
}

/// Translate a UI string into the phone's language.
fn t(c: &Core, s: &'static str) -> &'static str {
    i18n::tr(c.settings.language, s)
}

/// Translate a template and fill its `{}` placeholders.
fn tf(c: &Core, s: &'static str, args: &[&str]) -> String {
    i18n::fill(t(c, s), args)
}

/// Translate a runtime string if it's a known key.
fn td(c: &Core, s: &str) -> String {
    i18n::tr_dyn(c.settings.language, s)
}

fn on_off(c: &Core, b: bool) -> &'static str {
    t(c, if b { "On" } else { "Off" })
}

// ------------------------------------------------------------------ big clock font

const BIG: [[&str; 3]; 10] = [
    ["█▀█", "█ █", "█▄█"],
    ["▀█ ", " █ ", "▄█▄"],
    ["▀▀█", "█▀▀", "█▄▄"],
    ["▀▀█", " ▀█", "▄▄█"],
    ["█ █", "▀▀█", "  █"],
    ["█▀▀", "▀▀█", "▄▄█"],
    ["█▀▀", "█▀█", "█▄█"],
    ["▀▀█", "  █", "  █"],
    ["█▀█", "█▀█", "█▄█"],
    ["█▀█", "▀▀█", "▄▄█"],
];

fn big_time(s: &str) -> [String; 3] {
    let mut rows = [String::new(), String::new(), String::new()];
    for ch in s.chars() {
        for (r, row) in rows.iter_mut().enumerate() {
            match ch.to_digit(10) {
                Some(d) => row.push_str(BIG[d as usize][r]),
                None => row.push_str([" ", "▪", "▪"][r]),
            }
            row.push(' ');
        }
    }
    rows.map(|r| center(r.trim_end(), W))
}

// ------------------------------------------------------------------ generic chooser

pub struct Menu {
    title: String,
    items: Vec<String>,
    sel: usize,
    tag: u32,
}

impl Menu {
    pub fn new(title: &str, items: &[&str], tag: u32) -> Menu {
        Menu { title: title.into(), items: items.iter().map(|s| s.to_string()).collect(), sel: 0, tag }
    }
}

impl Screen for Menu {
    fn render(&self, c: &Core, f: &mut Frame) {
        f.title = td(c, &self.title);
        let items: Vec<String> = self.items.iter().enumerate().map(|(i, s)| format!("{} {}", i + 1, td(c, s))).collect();
        list(f, &items, self.sel, ROWS);
        f.soft("", t(c, "Select"), t(c, "Back"));
    }
    fn key(&mut self, _c: &mut Core, k: Key) -> Nav {
        if let Key::Num(d @ '1'..='9') = k {
            let i = d as usize - '1' as usize;
            if i < self.items.len() {
                return Nav::Result(i, self.tag);
            }
        }
        if nav_list(k, &mut self.sel, self.items.len()) {
            return Nav::Stay;
        }
        match k {
            Key::Ok => Nav::Result(self.sel, self.tag),
            Key::Back => Nav::Pop,
            _ => Nav::Stay,
        }
    }
}

// ------------------------------------------------------------------ message box

pub struct MsgBox {
    title: String,
    lines: Vec<String>,
    scroll: usize,
    alert: bool,
}

impl MsgBox {
    pub fn new(title: &str, text: &str) -> MsgBox {
        MsgBox { title: title.into(), lines: wrap(text, W), scroll: 0, alert: false }
    }
    pub fn lines(title: &str, lines: Vec<String>) -> MsgBox {
        MsgBox { title: title.into(), lines, scroll: 0, alert: false }
    }
    pub fn alert(title: &str, text: &str) -> MsgBox {
        let mut l = vec![String::new(), String::new()];
        l.extend(wrap(text, W));
        MsgBox { title: title.into(), lines: l, scroll: 0, alert: true }
    }
}

impl Screen for MsgBox {
    fn render(&self, c: &Core, f: &mut Frame) {
        f.title = if self.alert { td(c, &self.title).to_uppercase() } else { td(c, &self.title) };
        for (i, l) in self.lines.iter().enumerate().skip(self.scroll).take(ROWS) {
            if self.alert && i == 0 {
                f.line(&center(t(c, "⚠ EMERGENCY ALERT ⚠"), W));
            } else {
                f.line(l);
            }
        }
        let more = if self.lines.len() > self.scroll + ROWS { "▼" } else { "" };
        f.soft("", t(c, "OK"), more);
    }
    fn key(&mut self, _c: &mut Core, k: Key) -> Nav {
        match k {
            Key::Down if self.scroll + ROWS < self.lines.len() => self.scroll += 1,
            Key::Up if self.scroll > 0 => self.scroll -= 1,
            Key::Ok | Key::Back => return Nav::Pop,
            _ => {}
        }
        Nav::Stay
    }
}

// ------------------------------------------------------------------ PIN

#[derive(Default)]
pub struct PinEntry {
    pin: String,
    msg: String,
}

impl Screen for PinEntry {
    fn render(&self, c: &Core, f: &mut Frame) {
        f.title = t(c, "SIM locked").into();
        f.line("");
        f.line(&center(t(c, "Enter SIM PIN"), W));
        f.line("");
        f.line(&center(&"*".repeat(self.pin.len()), W));
        f.line("");
        f.line(&center(&tf(c, "{} attempts left", &[&c.pin_tries.to_string()]), W));
        f.line(&center(&td(c, &self.msg), W));
        f.line("");
        f.line(&center(t(c, "Emergency: dial 112"), W));
        f.soft("", t(c, "OK"), if self.pin.is_empty() { "" } else { t(c, "Clear") });
    }
    fn key(&mut self, c: &mut Core, k: Key) -> Nav {
        match k {
            Key::Num(d) if d.is_ascii_digit() && self.pin.len() < 8 => self.pin.push(d),
            Key::Back => {
                self.pin.pop();
            }
            Key::Call if Core::is_emergency(&self.pin) => {
                let n = core::mem::take(&mut self.pin);
                if let Err(e) = c.dial(&n) {
                    self.msg = e;
                } else {
                    return push(CallScreen::default());
                }
            }
            Key::Ok if self.pin.len() >= 4 => {
                let p = core::mem::take(&mut self.pin);
                if c.enter_pin(&p) {
                    return Nav::Pop;
                }
                self.msg = "Wrong PIN".into();
            }
            _ => {}
        }
        Nav::Stay
    }
}

// ------------------------------------------------------------------ home

pub struct Home;

impl Screen for Home {
    fn render(&self, c: &Core, f: &mut Frame) {
        let d = util::civil_from_unix(c.local());
        f.title = String::new();
        f.line("");
        for r in big_time(&hhmm(c.local())) {
            f.line(&r);
        }
        let l = c.settings.language;
        f.line(&center(&format!("{} {} {} {}", i18n::weekday(l, d.wday as usize), d.day, i18n::month(l, d.month as usize - 1), d.year), W));
        f.line("");
        let op = if c.settings.airplane {
            t(c, "Airplane mode").to_string()
        } else if c.sim_locked {
            t(c, "SIM locked").to_string()
        } else if c.net.registered {
            c.net.operator.clone()
        } else if c.net.searching {
            t(c, "Searching…").to_string()
        } else {
            t(c, "No service").to_string()
        };
        f.line(&center(&op, W));
        let mut notes = Vec::new();
        let unread = c.unread_total();
        if unread > 0 {
            notes.push(if unread == 1 { t(c, "✉ 1 new message").to_string() } else { tf(c, "✉ {} new messages", &[&unread.to_string()]) });
        }
        if c.missed > 0 {
            notes.push(if c.missed == 1 { t(c, "☎ 1 missed call").to_string() } else { tf(c, "☎ {} missed calls", &[&c.missed.to_string()]) });
        }
        if c.settings.alarm_on {
            if let Some((h, m)) = c.settings.alarm {
                notes.push(tf(c, "◷ Alarm {}", &[&format!("{:02}:{:02}", h, m)]));
            }
        }
        f.line("");
        for n in notes.iter().take(2) {
            f.line(&center(n, W));
        }
        f.soft(t(c, "Names"), t(c, "Menu"), "");
    }
    fn key(&mut self, c: &mut Core, k: Key) -> Nav {
        match k {
            Key::Ok => push(MainMenu::default()),
            Key::Opt | Key::Right => push(Contacts::default()),
            Key::Left => push(Threads::default()),
            Key::Up => push(ClockApp::default()),
            Key::Down | Key::Call => push(CallLog::default()),
            Key::Num(d) => push(Dialer { number: d.to_string() }),
            Key::Char('+') => push(Dialer { number: "+".into() }),
            Key::Char('m') => push(Threads::default()),
            _ => {
                let _ = c;
                Nav::Stay
            }
        }
    }
}

// ------------------------------------------------------------------ main menu

#[derive(Default)]
pub struct MainMenu {
    sel: usize,
}

const MENU: [&str; 8] = ["Messages", "Contacts", "Call log", "Clock & alarm", "Calculator", "Settings", "Network simulator", "Phone status"];

impl Screen for MainMenu {
    fn render(&self, c: &Core, f: &mut Frame) {
        f.title = t(c, "Menu").into();
        let items: Vec<String> = MENU
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let badge = match i {
                    0 if c.unread_total() > 0 => format!(" ({})", c.unread_total()),
                    2 if c.missed > 0 => format!(" ({})", c.missed),
                    _ => String::new(),
                };
                format!("{} {}{}", i + 1, t(c, m), badge)
            })
            .collect();
        list(f, &items, self.sel, ROWS);
        f.soft("", t(c, "Select"), t(c, "Back"));
    }
    fn key(&mut self, c: &mut Core, k: Key) -> Nav {
        let pick = match k {
            Key::Num(d @ '1'..='8') => Some(d as usize - '1' as usize),
            Key::Ok => Some(self.sel),
            Key::Back => return Nav::Pop,
            _ => {
                nav_list(k, &mut self.sel, MENU.len());
                None
            }
        };
        match pick {
            Some(0) => push(Threads::default()),
            Some(1) => push(Contacts::default()),
            Some(2) => push(CallLog::default()),
            Some(3) => push(ClockApp::default()),
            Some(4) => push(Calculator::default()),
            Some(5) => push(SettingsScreen::default()),
            Some(6) => push(Menu::new(
                "Simulator",
                &["Incoming call (Bob)", "SMS from Bob", "Chat from Alice", "MMS from Bob", "Emergency alert", "Lose signal 8s", "Long SMS (Carol)", "Group chat (Alice)"],
                1,
            )),
            Some(7) => push(MsgBox::lines("Phone status", status_lines(c))),
            _ => Nav::Stay,
        }
    }
    fn result(&mut self, c: &mut Core, i: usize, tag: u32) -> Nav {
        if tag == 1 {
            c.simulate(i as u32 + 1);
            c.toast("Simulated event sent");
        }
        Nav::Stay
    }
}

fn status_lines(c: &Core) -> Vec<String> {
    let n = &c.net;
    let signal = if n.rssi == 99 { "-".to_string() } else { format!("{} dBm, {}/4", -113 + 2 * n.rssi as i32, n.bars()) };
    let mut v = vec![
        tf(c, "Network: {}", &[if n.registered { n.operator.as_str() } else { t(c, "none") }]),
        format!("RAT: {} (AcT {})", n.rat(), n.act),
        tf(c, "Signal: {}", &[&signal]),
        tf(c, "Data: {}", &[if n.pdp_up { "internet, ims, mms" } else { t(c, "off") }]),
        format!("Chat (RCS): {}", t(c, c.rcs.state.label())),
        tf(c, "Battery: {}%", &[&n.battery.to_string()]),
        tf(c, "Number: {}", &[&c.info.number]),
        format!("IMEI: {}", c.info.imei),
        format!("IMSI: {}", c.info.imsi),
        format!("ICCID: {}", c.info.iccid),
        format!("Modem: {} {}", c.info.manufacturer, c.info.model),
        format!("FW: {}", c.info.firmware),
        tf(c, "MMS sent/recv: {}", &[&format!("{}/{}", c.mms.sent, c.mms.received)]),
        tf(c, "Saved data: {}", &[&td(c, &c.storage)]),
        tf(c, "Uptime: {}", &[&duration(c.now / 1000)]),
    ];
    v.iter_mut().for_each(|l| *l = trunc(l, W));
    v
}

// ------------------------------------------------------------------ dialer

pub struct Dialer {
    pub number: String,
}

impl Screen for Dialer {
    fn render(&self, c: &Core, f: &mut Frame) {
        f.title = t(c, "Dial").into();
        f.line("");
        f.line("");
        for l in wrap(&self.number, W - 2).iter().take(3) {
            f.line(&center(l, W));
        }
        f.line("");
        let name = c.contacts.iter().find(|x| same_number(&x.number, &self.number) && self.number.len() > 3).map(|x| x.name.clone());
        if let Some(n) = name {
            f.line(&center(&n, W));
        } else if is_mmi(&self.number) {
            f.line(&center(t(c, "Service code"), W));
        }
        f.soft(t(c, "Save"), t(c, "Call"), t(c, "Clear"));
    }
    fn key(&mut self, c: &mut Core, k: Key) -> Nav {
        match k {
            Key::Num(d) => {
                self.number.push(d);
                if self.number == "*#06#" {
                    return Nav::Replace(Box::new(MsgBox::new("IMEI", &c.info.imei.clone())));
                }
                if self.number.ends_with("**") {
                    self.number.truncate(self.number.len() - 2);
                    self.number.push('+');
                }
            }
            Key::Char(ch) if ch == '+' || ch == 'p' || ch == 'w' => self.number.push(ch),
            Key::Back => {
                self.number.pop();
                if self.number.is_empty() {
                    return Nav::Pop;
                }
            }
            Key::Ok | Key::Call => {
                if is_mmi(&self.number) {
                    c.ussd(&self.number);
                    c.toast("Sending request…");
                    return Nav::Pop;
                }
                let n = self.number.clone();
                return match c.dial(&n) {
                    Ok(()) => Nav::Replace(Box::new(CallScreen::default())),
                    Err(e) => {
                        let m = tf(c, "Call failed: {}", &[&td(c, &e)]);
                        c.toast(&m);
                        Nav::Stay
                    }
                };
            }
            Key::Opt => return Nav::Replace(Box::new(ContactEdit::new(None, "", &self.number))),
            _ => {}
        }
        Nav::Stay
    }
}

fn is_mmi(n: &str) -> bool {
    n.len() > 2 && (n.starts_with('*') || n.starts_with('#')) && n.ends_with('#')
}

// ------------------------------------------------------------------ in-call

#[derive(Default)]
pub struct CallScreen {
    dtmf: String,
}

impl Screen for CallScreen {
    fn is_call(&self) -> bool {
        true
    }
    fn render(&self, c: &Core, f: &mut Frame) {
        let call = match &c.call {
            Some(x) => x,
            None => {
                f.title = t(c, "Voice call").into();
                f.line(&center(t(c, "Call ended"), W));
                return;
            }
        };
        let name = if call.number.is_empty() { t(c, "Unknown").to_string() } else { c.name_for(&call.number) };
        let state = match call.state {
            CallState::Dialing => t(c, "Calling…").to_string(),
            CallState::Alerting => t(c, "Ringing…").to_string(),
            CallState::Incoming => t(c, "Incoming call").to_string(),
            CallState::Active => duration(call.connected_at.map(|t| (c.now - t) / 1000).unwrap_or(0)),
        };
        f.title = t(c, if call.emergency { "Emergency call" } else { "Voice call" }).into();
        f.line("");
        f.line(&center(&state, W));
        f.line("");
        f.line(&center(&name, W));
        if name != call.number {
            f.line(&center(&call.number, W));
        } else {
            f.line("");
        }
        f.line("");
        let mut flags = vec![if c.net.act == 7 { "HD VoLTE" } else { "HD VoNR" }];
        if call.muted {
            flags.push(t(c, "Muted"));
        }
        if call.speaker {
            flags.push(t(c, "Speaker"));
        }
        f.line(&center(&flags.join(" · "), W));
        f.line("");
        f.line(&center(&self.dtmf, W));
        if call.state == CallState::Incoming {
            f.soft(t(c, "Silence"), t(c, "Answer"), t(c, "Reject"));
        } else {
            f.soft(if call.muted { t(c, "Unmute") } else { t(c, "Mute") }, if call.speaker { t(c, "Handset") } else { t(c, "Speaker") }, t(c, "End"));
        }
    }
    fn key(&mut self, c: &mut Core, k: Key) -> Nav {
        let incoming = c.call.as_ref().is_some_and(|x| x.state == CallState::Incoming);
        match k {
            Key::Ok | Key::Call if incoming => c.answer(),
            Key::Back if incoming => c.hangup(),
            Key::Opt if incoming => {
                if let Some(x) = &mut c.call {
                    x.silenced = true;
                }
            }
            Key::Back => c.hangup(),
            Key::Opt => c.toggle_mute(),
            Key::Ok => {
                if let Some(x) = &mut c.call {
                    x.speaker = !x.speaker;
                }
            }
            Key::Num(d) => {
                self.dtmf.push(d);
                if self.dtmf.len() > W - 2 {
                    self.dtmf.remove(0);
                }
                c.dtmf(d);
            }
            _ => {}
        }
        Nav::Stay
    }
}

// ------------------------------------------------------------------ messaging

fn sorted_threads(c: &Core) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..c.threads.len()).filter(|&i| !c.threads[i].msgs.is_empty()).collect();
    idx.sort_by_key(|&i| core::cmp::Reverse(c.threads[i].last_time()));
    idx
}

#[derive(Default)]
pub struct Threads {
    sel: usize,
    pending_delete: Option<String>,
}

impl Screen for Threads {
    fn render(&self, c: &Core, f: &mut Frame) {
        f.title = t(c, "Messages").into();
        let mut items = vec![t(c, "✎ New message").to_string(), t(c, "✎ New group").to_string()];
        for i in sorted_threads(c) {
            let th = &c.threads[i];
            let last = th.msgs.last().unwrap();
            let mark = if th.unread() > 0 { "●" } else { " " };
            let snippet = if last.text.is_empty() && !last.attachments.is_empty() { t(c, "[picture]").to_string() } else { last.text.replace('\n', " ") };
            items.push(format!("{}{}: {}", mark, c.name_for(&th.number), snippet));
        }
        list(f, &items, self.sel, ROWS);
        f.soft(t(c, "Delete"), t(c, "Open"), t(c, "Back"));
    }
    fn key(&mut self, c: &mut Core, k: Key) -> Nav {
        let order = sorted_threads(c);
        let len = order.len() + 2;
        if self.sel >= len {
            self.sel = len - 1;
        }
        if nav_list(k, &mut self.sel, len) {
            return Nav::Stay;
        }
        match k {
            Key::Ok if self.sel == 0 => push(NewMessage::default()),
            Key::Ok if self.sel == 1 => push(NewGroup::default()),
            Key::Ok => push(ThreadView::new(&c.threads[order[self.sel - 2]].number)),
            Key::Opt if self.sel > 1 => {
                self.pending_delete = Some(c.threads[order[self.sel - 2]].number.clone());
                push(Menu::new("Delete conversation?", &["Delete", "Cancel"], 9))
            }
            Key::Back => Nav::Pop,
            _ => Nav::Stay,
        }
    }
    fn result(&mut self, c: &mut Core, i: usize, tag: u32) -> Nav {
        if let (9, 0, Some(n)) = (tag, i, self.pending_delete.take()) {
            c.delete_thread(&n);
            c.toast("Conversation deleted");
        }
        self.pending_delete = None;
        Nav::Stay
    }
}

#[derive(Default)]
pub struct NewMessage {
    number: String,
    sel: usize,
}

impl Screen for NewMessage {
    fn render(&self, c: &Core, f: &mut Frame) {
        f.title = t(c, "New message").into();
        f.line(&tf(c, "To: {}_", &[&self.number]));
        f.line(&"─".repeat(W));
        let items: Vec<String> = c.contacts.iter().map(|x| format!("{} {}", x.name, x.number)).collect();
        if self.number.is_empty() {
            list(f, &items, self.sel, ROWS - 2);
        } else {
            f.line(t(c, "Type a number, or clear"));
            f.line(t(c, "it to pick a contact."));
        }
        f.soft("", t(c, "Next"), t(c, "Back"));
    }
    fn key(&mut self, c: &mut Core, k: Key) -> Nav {
        match k {
            Key::Num(d) if d.is_ascii_digit() || d == '*' || d == '#' => self.number.push(d),
            Key::Char('+') => self.number.push('+'),
            Key::Back if !self.number.is_empty() => {
                self.number.pop();
            }
            Key::Back => return Nav::Pop,
            Key::Ok => {
                let to = if !self.number.is_empty() {
                    self.number.clone()
                } else if let Some(x) = c.contacts.get(self.sel) {
                    x.number.clone()
                } else {
                    return Nav::Stay;
                };
                c.rcs_query_caps(&to);
                return Nav::Replace(Box::new(Compose::new(&to)));
            }
            _ if self.number.is_empty() => {
                nav_list(k, &mut self.sel, c.contacts.len());
            }
            _ => {}
        }
        Nav::Stay
    }
}

/// Pick group members (OK toggles), then name the group.
#[derive(Default)]
pub struct NewGroup {
    sel: usize,
    picked: Vec<usize>,
    naming: Option<TextInput>,
}

impl Screen for NewGroup {
    fn render(&self, c: &Core, f: &mut Frame) {
        if let Some(name) = &self.naming {
            f.title = t(c, "Group name").into();
            for l in name.lines(W, c.now).iter().take(3) {
                f.line(l);
            }
            f.line("");
            f.line(&tf(c, "{} members + you", &[&self.picked.len().to_string()]));
            f.soft("", t(c, "Create"), name.mode_label());
            return;
        }
        f.title = tf(c, "New group ({})", &[&self.picked.len().to_string()]);
        let order = sorted_contacts(c);
        let items: Vec<String> = order.iter().map(|&i| format!("{} {}", if self.picked.contains(&i) { "[x]" } else { "[ ]" }, c.contacts[i].name)).collect();
        list(f, &items, self.sel, ROWS);
        f.soft(if self.picked.len() >= 2 { t(c, "Next") } else { "" }, t(c, "Select"), t(c, "Back"));
    }
    fn key(&mut self, c: &mut Core, k: Key) -> Nav {
        if let Some(name) = &mut self.naming {
            return match k {
                Key::Ok => {
                    let members: Vec<String> = self.picked.iter().filter_map(|&i| c.contacts.get(i).map(|x| x.number.clone())).collect();
                    let key = c.create_group(name.text.trim(), &members, None);
                    Nav::Replace(Box::new(Compose::new(&key)))
                }
                Key::Back if name.text.is_empty() => {
                    self.naming = None;
                    Nav::Stay
                }
                _ => {
                    name.lang = c.settings.language;
                    name.key(k, c.now);
                    Nav::Stay
                }
            };
        }
        let order = sorted_contacts(c);
        if nav_list(k, &mut self.sel, order.len()) {
            return Nav::Stay;
        }
        match k {
            Key::Ok => {
                if let Some(&i) = order.get(self.sel) {
                    match self.picked.iter().position(|&p| p == i) {
                        Some(p) => {
                            self.picked.remove(p);
                        }
                        None => {
                            self.picked.push(i);
                            let n = c.contacts[i].number.clone();
                            c.rcs_query_caps(&n);
                        }
                    }
                }
                Nav::Stay
            }
            Key::Opt if self.picked.len() >= 2 => {
                self.naming = Some(TextInput::new(Mode::Sentence));
                Nav::Stay
            }
            Key::Back => Nav::Pop,
            _ => Nav::Stay,
        }
    }
}

pub struct ThreadView {
    number: String,
    scroll: usize,
    caps_checked: bool,
}

impl ThreadView {
    pub fn new(number: &str) -> ThreadView {
        ThreadView { number: number.to_string(), scroll: 0, caps_checked: false }
    }
}

fn status_mark(c: &Core, m: &Message) -> &'static str {
    match m.status {
        Status::Sending => "…",
        Status::Sent => "✓",
        Status::Delivered => "✓✓",
        Status::Read => t(c, "Read"),
        Status::Failed => t(c, "! Failed"),
        Status::Received => "",
    }
}

pub fn attachment_lines(c: &Core, p: &Part) -> Vec<String> {
    if p.content_type.eq_ignore_ascii_case("image/bmp") {
        if let Some(rows) = bmp::render(&p.data) {
            return rows.iter().map(|r| center(r, W)).collect();
        }
    }
    if p.content_type.to_ascii_lowercase().contains("vcard") {
        let t = String::from_utf8_lossy(&p.data).into_owned();
        let get = |k: &str| t.lines().find(|l| l.starts_with(k)).and_then(|l| l.split_once(':')).map(|(_, v)| v.trim().to_string()).unwrap_or_default();
        return vec![tf(c, " [Contact] {}", &[&get("FN")]), format!("  {}", get("TEL"))];
    }
    vec![format!(" [{} {}B]", p.name, p.data.len())]
}

impl ThreadView {
    fn build(&self, c: &Core) -> Vec<String> {
        let mut out = Vec::new();
        let th = match c.threads.iter().find(|t| same_number(&t.number, &self.number)) {
            Some(t) => t,
            None => return out,
        };
        let name = c.name_for(&self.number);
        for m in &th.msgs {
            let sender = if th.is_group() && !m.from.is_empty() { c.name_for(&m.from) } else { name.clone() };
            let who = if m.incoming { sender.as_str() } else { t(c, "Me") };
            let when = hhmm(m.time + c.tz_q as i64 * 900);
            let head = if m.incoming { format!("{} {} {}", who, when, m.transport.label()) } else { format!("{} {} {} {}", who, when, m.transport.label(), status_mark(c, m)) };
            out.push(if m.incoming { head } else { format!("{:>w$}", head, w = W) });
            if let Some(s) = &m.subject {
                out.push(tf(c, " Subj: {}", &[s]));
            }
            if !m.text.is_empty() {
                for l in wrap(&m.text, W - 1) {
                    out.push(format!(" {}", l));
                }
            }
            for p in &m.attachments {
                out.extend(attachment_lines(c, p));
            }
        }
        if th.typing_until > c.now {
            let who = if th.is_group() && !th.typing_who.is_empty() { c.name_for(&th.typing_who) } else { name };
            out.push(tf(c, "{} is typing…", &[&who]));
        }
        out
    }
}

impl Screen for ThreadView {
    fn thread(&self) -> Option<&str> {
        Some(&self.number)
    }
    fn render(&self, c: &Core, f: &mut Frame) {
        let name = c.name_for(&self.number);
        let tag = match c.planned_transport(&self.number, false, false) {
            Transport::Rcs => " · Chat",
            Transport::Mms => " · MMS",
            Transport::Sms => " · SMS",
        };
        f.title = trunc(&format!("{}{}", name, tag), W);
        let lines = self.build(c);
        let start = lines.len().saturating_sub(ROWS + self.scroll);
        for l in lines.iter().skip(start).take(ROWS) {
            f.line(l);
        }
        f.soft(t(c, "Options"), t(c, "Write"), t(c, "Back"));
    }
    fn key(&mut self, c: &mut Core, k: Key) -> Nav {
        match k {
            Key::Up => {
                if self.build(c).len() > ROWS + self.scroll {
                    self.scroll += 1;
                }
            }
            Key::Down => self.scroll = self.scroll.saturating_sub(1),
            Key::Ok => return push(Compose::reply(&self.number)),
            Key::Call => {
                let n = self.number.clone();
                return match c.dial(&n) {
                    Ok(()) => push(CallScreen::default()),
                    Err(e) => {
                        c.toast(&e);
                        Nav::Stay
                    }
                };
            }
            Key::Opt if self.number.starts_with(crate::services::GROUP_PREFIX) => return push(Menu::new("Group", &["Reply", "Members", "Delete conversation"], 5)),
            Key::Opt => return push(Menu::new("Options", &["Reply", "Call", "Check chat features", "Delete conversation"], 2)),
            Key::Back => return Nav::Pop,
            _ => {}
        }
        Nav::Stay
    }
    fn result(&mut self, c: &mut Core, i: usize, tag: u32) -> Nav {
        let n = self.number.clone();
        if tag == 5 {
            return match i {
                0 => push(Compose::reply(&n)),
                1 => {
                    let mut lines = vec![t(c, "You").to_string()];
                    if let Some(t) = c.thread(&n) {
                        for m in &t.members {
                            let chat = if c.peer_rcs(m) == Some(true) { " ◆" } else { "" };
                            lines.push(trunc(&format!("{} {}{}", c.name_for(m), m, chat), W));
                        }
                    }
                    lines.push(String::new());
                    lines.push(t(c, "◆ = can use chat").into());
                    push(MsgBox::lines("Members", lines))
                }
                _ => {
                    c.delete_thread(&n);
                    Nav::Pop
                }
            };
        }
        match i {
            0 => push(Compose::reply(&n)),
            1 => match c.dial(&n) {
                Ok(()) => push(CallScreen::default()),
                Err(e) => {
                    c.toast(&e);
                    Nav::Stay
                }
            },
            2 => {
                c.rcs.caps.retain(|k, _| !same_number(k, &n));
                c.rcs_query_caps(&n);
                c.toast("Checking…");
                Nav::Stay
            }
            3 => {
                c.delete_thread(&n);
                Nav::Pop
            }
            _ => Nav::Stay,
        }
    }
    fn tick(&mut self, c: &mut Core) -> Nav {
        if !self.caps_checked && c.rcs_usable() {
            self.caps_checked = true;
            if !self.number.starts_with(crate::services::GROUP_PREFIX) && c.peer_rcs(&self.number).is_none() {
                let n = self.number.clone();
                c.rcs_query_caps(&n);
            }
        }
        let unread = c.threads.iter().any(|t| same_number(&t.number, &self.number) && t.unread() > 0);
        if unread {
            let n = self.number.clone();
            c.mark_read(&n);
            self.scroll = 0;
        }
        Nav::Stay
    }
}

pub struct Compose {
    to: String,
    /// opened from the conversation itself: return to it instead of opening another
    reply: bool,
    input: TextInput,
    atts: Vec<Part>,
    force_sms: bool,
    last_len: usize,
}

impl Compose {
    pub fn reply(to: &str) -> Compose {
        let mut c = Compose::new(to);
        c.reply = true;
        c
    }

    pub fn new(to: &str) -> Compose {
        Compose { to: to.to_string(), reply: false, input: TextInput::new(Mode::Sentence), atts: Vec::new(), force_sms: false, last_len: 0 }
    }

    fn transport(&self, c: &Core) -> Transport {
        c.planned_transport(&self.to, !self.atts.is_empty(), self.force_sms)
    }
}

impl Screen for Compose {
    fn thread(&self) -> Option<&str> {
        Some(&self.to)
    }
    fn render(&self, c: &Core, f: &mut Frame) {
        f.title = trunc(&tf(c, "To {}", &[&c.name_for(&self.to)]), W);
        let info = match self.transport(c) {
            Transport::Sms => {
                let (coding, segs) = sms::segment_text(&self.input.text);
                let per = match (coding, segs.len() > 1) {
                    (sms::Coding::Gsm7, false) => 160,
                    (sms::Coding::Gsm7, true) => 153,
                    (_, false) => 70,
                    (_, true) => 67,
                };
                let used = segs.last().map(|s| if coding == sms::Coding::Gsm7 { crate::gsm7::encode(s).map(|v| v.len()).unwrap_or(0) } else { s.encode_utf16().count() }).unwrap_or(0);
                format!("SMS {}/{} ({})", used, per, segs.len().max(1))
            }
            Transport::Mms if self.to.starts_with(crate::services::GROUP_PREFIX) => t(c, "Group MMS").to_string(),
            Transport::Mms if self.atts.len() == 1 => t(c, "MMS · 1 attachment").to_string(),
            Transport::Mms => tf(c, "MMS · {} attachments", &[&self.atts.len().to_string()]),
            Transport::Rcs if self.to.starts_with(crate::services::GROUP_PREFIX) => t(c, "Group chat").to_string(),
            Transport::Rcs if !self.atts.is_empty() => t(c, "Chat · file").to_string(),
            Transport::Rcs => t(c, "Chat message").to_string(),
        };
        f.line(&format!("{:>w$}", format!("{} {}", info, self.input.mode_label()), w = W));
        let mut lines = self.input.lines(W, c.now);
        for a in &self.atts {
            lines.push(format!("[+ {}]", a.name));
        }
        let start = lines.len().saturating_sub(ROWS - 1);
        for l in lines.iter().skip(start) {
            f.line(l);
        }
        f.soft(t(c, "Options"), t(c, "Send"), if self.input.text.is_empty() { t(c, "Back") } else { t(c, "Clear") });
    }
    fn key(&mut self, c: &mut Core, k: Key) -> Nav {
        match k {
            Key::Ok => return self.send(c),
            Key::Opt => {
                return push(Menu::new(
                    "Options",
                    &["Send", "Insert picture: smiley", "Insert picture: heart", "Insert contact card", if self.force_sms { "Use chat if available" } else { "Send as SMS" }, "Remove attachments"],
                    3,
                ))
            }
            Key::Back if self.input.text.is_empty() => {
                c.rcs_composing(&self.to.clone(), false);
                return Nav::Pop;
            }
            _ => {
                self.input.lang = c.settings.language;
                self.input.key(k, c.now);
            }
        }
        let len = self.input.text.len();
        if len != self.last_len {
            self.last_len = len;
            let to = self.to.clone();
            c.rcs_composing(&to, len > 0);
        }
        Nav::Stay
    }
    fn result(&mut self, c: &mut Core, i: usize, _tag: u32) -> Nav {
        match i {
            0 => return self.send(c),
            1 => self.atts.push(Part { content_type: "image/bmp".into(), name: "smiley.bmp".into(), data: bmp::encode(&bmp::SMILEY) }),
            2 => self.atts.push(Part { content_type: "image/bmp".into(), name: "heart.bmp".into(), data: bmp::encode(&bmp::HEART) }),
            3 => {
                let vcf = format!("BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Me\r\nTEL;TYPE=CELL:{}\r\nEND:VCARD\r\n", c.info.number);
                self.atts.push(Part { content_type: "text/x-vcard".into(), name: "me.vcf".into(), data: vcf.into_bytes() });
            }
            4 => self.force_sms = !self.force_sms,
            5 => self.atts.clear(),
            _ => {}
        }
        Nav::Stay
    }
}

impl Compose {
    fn send(&mut self, c: &mut Core) -> Nav {
        if self.input.text.trim().is_empty() && self.atts.is_empty() {
            return Nav::Stay;
        }
        let text = self.input.text.clone();
        c.send_message(&self.to, &text, core::mem::take(&mut self.atts), self.force_sms);
        if self.reply {
            Nav::Pop
        } else {
            Nav::Replace(Box::new(ThreadView::new(&self.to)))
        }
    }
}

// ------------------------------------------------------------------ contacts

fn sorted_contacts(c: &Core) -> Vec<usize> {
    let mut v: Vec<usize> = (0..c.contacts.len()).collect();
    v.sort_by_key(|&i| c.contacts[i].name.to_ascii_lowercase());
    v
}

#[derive(Default)]
pub struct Contacts {
    sel: usize,
}

impl Screen for Contacts {
    fn render(&self, c: &Core, f: &mut Frame) {
        f.title = t(c, "Contacts").into();
        let mut items = vec![t(c, "+ Add contact").to_string()];
        for i in sorted_contacts(c) {
            let x = &c.contacts[i];
            let badge = if c.peer_rcs(&x.number) == Some(true) { " ◆" } else { "" };
            items.push(format!("{}{}", x.name, badge));
        }
        list(f, &items, self.sel, ROWS);
        f.soft(t(c, "Message"), t(c, "Open"), t(c, "Back"));
    }
    fn key(&mut self, c: &mut Core, k: Key) -> Nav {
        let order = sorted_contacts(c);
        let len = order.len() + 1;
        self.sel = self.sel.min(len - 1);
        if nav_list(k, &mut self.sel, len) {
            return Nav::Stay;
        }
        let cur = if self.sel > 0 { order.get(self.sel - 1).copied() } else { None };
        match (k, cur) {
            (Key::Ok, None) => push(ContactEdit::new(None, "", "")),
            (Key::Ok, Some(i)) => push(ContactView { idx: i, sel: 0 }),
            (Key::Call, Some(i)) => {
                let n = c.contacts[i].number.clone();
                match c.dial(&n) {
                    Ok(()) => push(CallScreen::default()),
                    Err(e) => {
                        c.toast(&e);
                        Nav::Stay
                    }
                }
            }
            (Key::Opt, Some(i)) => push(Compose::new(&c.contacts[i].number.clone())),
            (Key::Back, _) => Nav::Pop,
            _ => Nav::Stay,
        }
    }
}

pub struct ContactView {
    idx: usize,
    sel: usize,
}

const CONTACT_ACTIONS: [&str; 5] = ["Call", "Send message", "Check chat features", "Edit", "Delete"];

impl Screen for ContactView {
    fn render(&self, c: &Core, f: &mut Frame) {
        let x = match c.contacts.get(self.idx) {
            Some(x) => x,
            None => return,
        };
        f.title = x.name.clone();
        f.line(&center(&x.number, W));
        let chat = t(c, match c.peer_rcs(&x.number) {
            Some(true) => "Chat: available",
            Some(false) => "Chat: not available",
            None => "Chat: unknown",
        });
        f.line(&center(chat, W));
        f.line(&"─".repeat(W));
        let items: Vec<String> = CONTACT_ACTIONS.iter().map(|s| t(c, s).to_string()).collect();
        list(f, &items, self.sel, ROWS - 3);
        f.soft("", t(c, "Select"), t(c, "Back"));
    }
    fn key(&mut self, c: &mut Core, k: Key) -> Nav {
        // number keys pick an action directly, like in menus
        let k = match k {
            Key::Num(d @ '1'..='5') => {
                self.sel = d as usize - '1' as usize;
                Key::Ok
            }
            k => k,
        };
        if nav_list(k, &mut self.sel, CONTACT_ACTIONS.len()) {
            return Nav::Stay;
        }
        let x = match c.contacts.get(self.idx) {
            Some(x) => x.clone(),
            None => return Nav::Pop,
        };
        let act = match k {
            Key::Ok => self.sel,
            Key::Call => 0,
            Key::Back => return Nav::Pop,
            _ => return Nav::Stay,
        };
        match act {
            0 => match c.dial(&x.number) {
                Ok(()) => push(CallScreen::default()),
                Err(e) => {
                    c.toast(&e);
                    Nav::Stay
                }
            },
            1 => push(Compose::new(&x.number)),
            2 => {
                c.rcs.caps.retain(|k, _| !same_number(k, &x.number));
                c.rcs_query_caps(&x.number);
                if !c.rcs_usable() {
                    c.toast("Chat features are off");
                }
                Nav::Stay
            }
            3 => Nav::Replace(Box::new(ContactEdit::new(Some(self.idx), &x.name, &x.number))),
            _ => push(Menu::new("Delete contact?", &["Delete", "Cancel"], 8)),
        }
    }
    fn result(&mut self, c: &mut Core, i: usize, tag: u32) -> Nav {
        if tag == 8 && i == 0 && self.idx < c.contacts.len() {
            c.contacts.remove(self.idx);
            c.toast("Contact deleted");
            return Nav::Pop;
        }
        Nav::Stay
    }
}

pub struct ContactEdit {
    idx: Option<usize>,
    field: usize,
    name: TextInput,
    number: TextInput,
}

impl ContactEdit {
    pub fn new(idx: Option<usize>, name: &str, number: &str) -> ContactEdit {
        let mut n = TextInput::with(number, Mode::Num);
        n.max = 20;
        let mut nm = TextInput::with(name, Mode::Sentence);
        nm.max = 30;
        ContactEdit { idx, field: if name.is_empty() { 0 } else { 1 }, name: nm, number: n }
    }
}

impl Screen for ContactEdit {
    fn render(&self, c: &Core, f: &mut Frame) {
        f.title = t(c, if self.idx.is_some() { "Edit contact" } else { "New contact" }).into();
        f.line(&tf(c, "Name: ({})", &[self.name.mode_label()]));
        for l in self.name.lines(W - 1, if self.field == 0 { c.now } else { u64::MAX }).iter().take(2) {
            f.line(&format!(" {}", if self.field == 0 { l.clone() } else { l.trim_end_matches('_').to_string() }));
        }
        f.line("");
        f.line(t(c, "Number:"));
        f.line(&format!(" {}{}", self.number.text, if self.field == 1 { "_" } else { "" }));
        f.sel = Some(if self.field == 0 { 1 } else { 4 });
        f.soft("", t(c, "Save"), t(c, "Clear"));
    }
    fn key(&mut self, c: &mut Core, k: Key) -> Nav {
        match k {
            Key::Up | Key::Down => self.field ^= 1,
            Key::Ok => {
                let name = self.name.text.trim().to_string();
                let number = util::normalize_number(&self.number.text);
                if name.is_empty() || number.is_empty() {
                    c.toast("Name and number needed");
                    return Nav::Stay;
                }
                match self.idx.and_then(|i| c.contacts.get_mut(i)) {
                    Some(x) => {
                        x.name = name;
                        x.number = number;
                    }
                    None => c.contacts.push(Contact { name, number }),
                }
                c.toast("Contact saved");
                return Nav::Pop;
            }
            _ => {
                let inp = if self.field == 0 { &mut self.name } else { &mut self.number };
                inp.lang = c.settings.language;
                let k = match k {
                    Key::Char('+') if self.field == 1 => Key::Char('+'),
                    Key::Char(_) if self.field == 1 => return Nav::Stay,
                    k => k,
                };
                if !inp.key(k, c.now) && k == Key::Back {
                    return Nav::Pop;
                }
            }
        }
        Nav::Stay
    }
}

// ------------------------------------------------------------------ call log

#[derive(Default)]
pub struct CallLog {
    sel: usize,
}

impl Screen for CallLog {
    fn render(&self, c: &Core, f: &mut Frame) {
        f.title = t(c, "Call log").into();
        if c.log.is_empty() {
            f.line("");
            f.line(&center(t(c, "No calls yet"), W));
        }
        let items: Vec<String> = c
            .log
            .iter()
            .rev()
            .map(|e| {
                let icon = match e.kind {
                    LogKind::Dialed => "↗",
                    LogKind::Received => "↙",
                    LogKind::Missed => "✗",
                };
                let t = hhmm(e.time + c.tz_q as i64 * 900);
                let name = trunc(&c.name_for(&e.number), W - 9);
                format!("{} {:<w$} {}", icon, name, t, w = W - 9)
            })
            .collect();
        list(f, &items, self.sel, ROWS);
        f.soft(t(c, "Message"), t(c, "Call"), t(c, "Back"));
    }
    fn tick(&mut self, c: &mut Core) -> Nav {
        c.missed = 0;
        Nav::Stay
    }
    fn key(&mut self, c: &mut Core, k: Key) -> Nav {
        let len = c.log.len();
        if nav_list(k, &mut self.sel, len) {
            return Nav::Stay;
        }
        let num = c.log.iter().rev().nth(self.sel).map(|e| e.number.clone());
        match (k, num) {
            (Key::Ok | Key::Call, Some(n)) => match c.dial(&n) {
                Ok(()) => push(CallScreen::default()),
                Err(e) => {
                    c.toast(&e);
                    Nav::Stay
                }
            },
            (Key::Opt, Some(n)) => push(Compose::new(&n)),
            (Key::Back, _) => Nav::Pop,
            _ => Nav::Stay,
        }
    }
}

// ------------------------------------------------------------------ clock

#[derive(Default)]
pub struct ClockApp {
    sel: usize,
}

impl Screen for ClockApp {
    fn render(&self, c: &Core, f: &mut Frame) {
        f.title = t(c, "Clock").into();
        let d = util::civil_from_unix(c.local());
        for r in big_time(&hhmm(c.local())) {
            f.line(&r);
        }
        f.line(&center(&format!("{:02}:{:02}:{:02}  UTC{:+}", d.hour, d.min, d.sec, c.tz_q as i32 / 4), W));
        f.line("");
        let (h, m) = c.settings.alarm.unwrap_or((7, 0));
        let items = vec![
            tf(c, "Alarm {}  [{}]", &[&format!("{:02}:{:02}", h, m), on_off(c, c.settings.alarm_on)]),
            t(c, "Set alarm time").to_string(),
            t(c, "Stopwatch").to_string(),
        ];
        list(f, &items, self.sel, 5);
        f.soft("", t(c, "Select"), t(c, "Back"));
    }
    fn key(&mut self, c: &mut Core, k: Key) -> Nav {
        if nav_list(k, &mut self.sel, 3) {
            return Nav::Stay;
        }
        match k {
            Key::Ok => match self.sel {
                0 => {
                    c.settings.alarm_on = !c.settings.alarm_on;
                    Nav::Stay
                }
                1 => push(AlarmSet::default()),
                _ => push(Stopwatch::default()),
            },
            Key::Back => Nav::Pop,
            _ => Nav::Stay,
        }
    }
}

#[derive(Default)]
pub struct AlarmSet {
    digits: String,
}

impl Screen for AlarmSet {
    fn render(&self, c: &Core, f: &mut Frame) {
        f.title = t(c, "Set alarm").into();
        let mut s: Vec<char> = "--:--".chars().collect();
        for (i, d) in self.digits.chars().enumerate() {
            s[if i < 2 { i } else { i + 1 }] = d;
        }
        f.line("");
        for r in big_time(&s.iter().map(|&c| if c == '-' { '0' } else { c }).collect::<String>()) {
            f.line(&r);
        }
        f.line(&center(&s.iter().collect::<String>(), W));
        f.line("");
        f.line(&center(t(c, "Type HHMM (24h)"), W));
        f.soft("", t(c, "Save"), t(c, "Clear"));
    }
    fn key(&mut self, c: &mut Core, k: Key) -> Nav {
        match k {
            Key::Num(d) if d.is_ascii_digit() && self.digits.len() < 4 => self.digits.push(d),
            Key::Back => {
                if self.digits.pop().is_none() {
                    return Nav::Pop;
                }
            }
            Key::Ok if self.digits.len() == 4 => {
                let h: u32 = self.digits[..2].parse().unwrap_or(99);
                let m: u32 = self.digits[2..].parse().unwrap_or(99);
                if h > 23 || m > 59 {
                    c.toast("Invalid time");
                    return Nav::Stay;
                }
                c.settings.alarm = Some((h, m));
                c.settings.alarm_on = true;
                let m = tf(c, "Alarm set for {}", &[&format!("{:02}:{:02}", h, m)]);
                c.toast(&m);
                return Nav::Pop;
            }
            _ => {}
        }
        Nav::Stay
    }
}

pub struct AlarmRing;

impl Screen for AlarmRing {
    fn is_alarm(&self) -> bool {
        true
    }
    fn render(&self, c: &Core, f: &mut Frame) {
        f.title = t(c, "Alarm").into();
        f.line("");
        for r in big_time(&hhmm(c.local())) {
            f.line(&r);
        }
        f.line("");
        let w = t(c, "WAKE UP");
        f.line(&center(&if (c.now / 500) % 2 == 0 { format!("♪ ♫ {} ♫ ♪", w) } else { format!("♫ ♪ {} ♪ ♫", w) }, W));
        f.soft(t(c, "Snooze"), t(c, "Stop"), "");
    }
    fn key(&mut self, c: &mut Core, k: Key) -> Nav {
        match k {
            Key::Opt => {
                c.alarm_ringing = false;
                c.alarm_snooze = Some(c.local() + 300);
                c.toast("Snoozed for 5 minutes");
                Nav::Pop
            }
            Key::Ok | Key::Back => {
                c.alarm_ringing = false;
                Nav::Pop
            }
            _ => Nav::Stay,
        }
    }
}

#[derive(Default)]
pub struct Stopwatch {
    start: Option<u64>,
    acc: u64,
    laps: Vec<u64>,
}

impl Stopwatch {
    fn elapsed(&self, now: u64) -> u64 {
        self.acc + self.start.map(|s| now - s).unwrap_or(0)
    }
}

fn fmt_sw(ms: u64) -> String {
    format!("{:02}:{:02}.{}", ms / 60000, ms / 1000 % 60, ms / 100 % 10)
}

impl Screen for Stopwatch {
    fn render(&self, c: &Core, f: &mut Frame) {
        f.title = t(c, "Stopwatch").into();
        let e = self.elapsed(c.now);
        f.line("");
        f.line(&center(&fmt_sw(e), W));
        f.line("");
        for (i, l) in self.laps.iter().enumerate().rev().take(6) {
            f.line(&center(&tf(c, "Lap {}  {}", &[&(i + 1).to_string(), &fmt_sw(*l)]), W));
        }
        f.soft(if self.start.is_some() { t(c, "Lap") } else { t(c, "Reset") }, if self.start.is_some() { t(c, "Stop") } else { t(c, "Start") }, t(c, "Back"));
    }
    fn key(&mut self, c: &mut Core, k: Key) -> Nav {
        match k {
            Key::Ok => match self.start.take() {
                Some(s) => self.acc += c.now - s,
                None => self.start = Some(c.now),
            },
            Key::Opt if self.start.is_some() => self.laps.push(self.elapsed(c.now)),
            Key::Opt => {
                self.acc = 0;
                self.laps.clear();
            }
            Key::Back => return Nav::Pop,
            _ => {}
        }
        Nav::Stay
    }
}

// ------------------------------------------------------------------ calculator

#[derive(Default)]
pub struct Calculator {
    expr: String,
    result: Option<String>,
}

fn eval(expr: &str) -> Option<f64> {
    // tokens: numbers and + - × ÷ ; precedence: × ÷ over + -
    let mut nums: Vec<f64> = Vec::new();
    let mut ops: Vec<char> = Vec::new();
    let mut cur = String::new();
    for ch in expr.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            cur.push(ch);
        } else if cur.is_empty() && ch == '-' && nums.len() == ops.len() {
            cur.push('-');
        } else {
            nums.push(cur.parse().ok()?);
            cur.clear();
            ops.push(ch);
        }
    }
    nums.push(cur.parse().ok()?);
    let mut terms = vec![nums[0]];
    let mut add_ops = Vec::new();
    for (i, &op) in ops.iter().enumerate() {
        let n = nums[i + 1];
        match op {
            '×' => *terms.last_mut()? *= n,
            '÷' => {
                if n == 0.0 {
                    return None;
                }
                *terms.last_mut()? /= n
            }
            _ => {
                terms.push(n);
                add_ops.push(op);
            }
        }
    }
    let mut acc = terms[0];
    for (i, op) in add_ops.iter().enumerate() {
        if *op == '+' {
            acc += terms[i + 1];
        } else {
            acc -= terms[i + 1];
        }
    }
    Some(acc)
}

fn fmt_num(v: f64) -> String {
    if v.is_nan() || v.is_infinite() {
        return "Error".into();
    }
    let r = (v * 1e9) as i128;
    if r % 1_000_000_000 == 0 && v.abs() < 1e15 {
        return format!("{}", r / 1_000_000_000);
    }
    let s = format!("{:.9}", v);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl Screen for Calculator {
    fn render(&self, c: &Core, f: &mut Frame) {
        f.title = t(c, "Calculator").into();
        f.line("");
        for l in wrap(if self.expr.is_empty() { "0" } else { &self.expr }, W - 2).iter().rev().take(3).rev() {
            f.line(&format!("{:>w$}", l, w = W - 1));
        }
        let r = self.result.as_deref().map(|r| if r == "Error" { t(c, "Error").to_string() } else { format!("= {}", r) }).unwrap_or_default();
        f.line(&format!("{:>w$}", r, w = W - 1));
        f.line("");
        f.line("  ↑ +    ↓ −    * .");
        f.line("  ← ×    → ÷    OK =");
        f.soft("", "=", t(c, "Clear"));
    }
    fn key(&mut self, _c: &mut Core, k: Key) -> Nav {
        let op = |e: &mut String, o: char| {
            if let Some(last) = e.chars().last() {
                if "+-×÷".contains(last) {
                    e.pop();
                }
                e.push(o);
            } else if o == '-' {
                e.push('-');
            }
        };
        if let Some(r) = self.result.take() {
            // continue from the previous result when an operator is pressed
            if matches!(k, Key::Up | Key::Down | Key::Left | Key::Right) && r != "Error" {
                self.expr = r;
            } else if !matches!(k, Key::Back) {
                self.expr.clear();
            }
        }
        match k {
            Key::Num(d) if d.is_ascii_digit() => self.expr.push(d),
            Key::Num('*') | Key::Char('.') => self.expr.push('.'),
            Key::Up | Key::Char('+') => op(&mut self.expr, '+'),
            Key::Down | Key::Char('-') => op(&mut self.expr, '-'),
            Key::Left | Key::Char('x') => op(&mut self.expr, '×'),
            Key::Right | Key::Char('/') => op(&mut self.expr, '÷'),
            Key::Ok | Key::Num('#') | Key::Char('=') => {
                if !self.expr.is_empty() {
                    self.result = Some(eval(&self.expr).map(fmt_num).unwrap_or_else(|| "Error".into()));
                }
            }
            Key::Back => {
                if self.expr.pop().is_none() {
                    return Nav::Pop;
                }
            }
            _ => {}
        }
        Nav::Stay
    }
}

// ------------------------------------------------------------------ settings

#[derive(Default)]
pub struct SettingsScreen {
    sel: usize,
}

const SETTINGS_COUNT: usize = 14;

impl SettingsScreen {
    fn items(c: &Core) -> Vec<String> {
        let s = &c.settings;
        vec![
            tf(c, "Language: {}", &[s.language.native_name()]),
            tf(c, "Network: {}", &[t(c, s.net_mode.label())]),
            tf(c, "Airplane mode: {}", &[on_off(c, s.airplane)]),
            tf(c, "Chat features: {}", &[on_off(c, s.rcs)]),
            tf(c, " Status: {}", &[t(c, c.rcs.state.label())]),
            tf(c, "Delivery reports: {}", &[on_off(c, s.delivery_reports)]),
            tf(c, "Read receipts: {}", &[on_off(c, s.read_receipts)]),
            tf(c, "Typing indicator: {}", &[on_off(c, s.typing_indicators)]),
            tf(c, "Ringtone: {}", &[on_off(c, s.ringer)]),
            t(c, "Phone status").to_string(),
            t(c, "AT command log").to_string(),
            t(c, "About").to_string(),
            t(c, "Restart phone").to_string(),
            t(c, "Power off").to_string(),
        ]
    }
}

impl Screen for SettingsScreen {
    fn render(&self, c: &Core, f: &mut Frame) {
        f.title = t(c, "Settings").into();
        list(f, &Self::items(c), self.sel, ROWS);
        f.soft("", t(c, "Change"), t(c, "Back"));
    }
    fn key(&mut self, c: &mut Core, k: Key) -> Nav {
        if nav_list(k, &mut self.sel, SETTINGS_COUNT) {
            return Nav::Stay;
        }
        if k == Key::Back {
            return Nav::Pop;
        }
        if k != Key::Ok {
            return Nav::Stay;
        }
        let s = &mut c.settings;
        match self.sel {
            0 => {
                let names: Vec<&str> = Lang::ALL.iter().map(|l| l.native_name()).collect();
                return push(Menu::new("Language", &names, 10));
            }
            1 => return push(Menu::new("Network mode", &["5G/4G auto", "4G only", "5G only"], 4)),
            2 => {
                let on = !s.airplane;
                c.set_airplane(on);
            }
            3 => {
                let on = !s.rcs;
                c.set_rcs(on);
            }
            4 => {}
            5 => s.delivery_reports = !s.delivery_reports,
            6 => s.read_receipts = !s.read_receipts,
            7 => s.typing_indicators = !s.typing_indicators,
            8 => s.ringer = !s.ringer,
            9 => return push(MsgBox::lines("Phone status", status_lines(c))),
            10 => {
                let lines: Vec<String> = c.modem.trace.iter().rev().take(40).flat_map(|l| wrap(l, W)).collect();
                return push(MsgBox::lines("AT command log", lines));
            }
            11 => return push(MsgBox::new("About", t(c, i18n::ABOUT))),
            12 => return push(Menu::new("Restart phone?", &["Restart", "Cancel"], 6)),
            _ => return push(Menu::new("Power off?", &["Power off", "Cancel"], 7)),
        }
        Nav::Stay
    }
    fn result(&mut self, c: &mut Core, i: usize, tag: u32) -> Nav {
        match (tag, i) {
            (4, _) => c.set_net_mode([NetMode::Auto5g, NetMode::Lte, NetMode::Nr][i.min(2)]),
            (6, 0) => c.power_request = Some(crate::services::Power::Restart),
            (7, 0) => c.power_request = Some(crate::services::Power::Off),
            (10, _) => c.settings.language = Lang::ALL[i.min(Lang::ALL.len() - 1)],
            _ => {}
        }
        Nav::Stay
    }
}

#[cfg(test)]
mod t {
    #[test]
    fn calc() {
        assert_eq!(super::eval("2+3×4"), Some(14.0));
        assert_eq!(super::eval("10÷4-1"), Some(1.5));
        assert_eq!(super::eval("-3×-2"), Some(6.0));
        assert_eq!(super::eval("1÷0"), None);
        assert_eq!(super::fmt_num(2.5), "2.5");
        assert_eq!(super::fmt_num(14.0), "14");
    }
}
