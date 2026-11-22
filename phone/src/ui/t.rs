//! UI-level tests: drive the phone with key presses like a user would.
use super::*;
use crate::sim::SimModem;
use std::cell::Cell;

thread_local!(static NOW: Cell<u64> = const { Cell::new(1) });
fn clock() -> u64 {
    NOW.with(|c| c.get())
}
fn run(p: &mut Phone, ms: u64) {
    for _ in 0..ms / 10 {
        NOW.with(|n| n.set(n.get() + 10));
        p.step(clock());
    }
}
fn keys(p: &mut Phone, ks: &[Key]) {
    for &k in ks {
        p.key(k);
        run(p, 50);
    }
}
fn typed(p: &mut Phone, s: &str) {
    for ch in s.chars() {
        let k = if ch.is_ascii_digit() || ch == '*' || ch == '#' { Key::Num(ch) } else { Key::Char(ch) };
        p.key(k);
        run(p, 20);
    }
}
fn screen(p: &Phone) -> String {
    let f = p.render();
    let mut s = format!("{}\n{}\n", f.status, f.title);
    for l in &f.lines {
        s.push_str(l);
        s.push('\n');
    }
    s
}
fn phone() -> Phone {
    NOW.with(|n| n.set(1));
    let mut p = Phone::new(Box::new(SimModem::new()), clock);
    p.boot();
    assert!(screen(&p).contains("Enter SIM PIN"));
    typed(&mut p, "1234");
    keys(&mut p, &[Key::Ok]);
    run(&mut p, 5000);
    p
}

#[test]
fn home_and_status_bar() {
    let p = phone();
    let s = screen(&p);
    assert!(s.contains("Tranquil Mobile"), "{}", s);
    assert!(s.starts_with("5G▂"), "{}", s);
    assert!(s.contains("Chat"), "{}", s);
    let a = p.render_ansi();
    assert!(a.contains("┌────────────────────────┐"));
    for line in a.split("\r\n").filter(|l| l.starts_with('│')) {
        let plain: String = strip_ansi(line);
        assert_eq!(crate::util::char_len(&plain), W + 2, "{:?}", plain);
    }
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut esc = false;
    for c in s.chars() {
        if esc {
            if c.is_ascii_alphabetic() {
                esc = false;
            }
        } else if c == '\x1b' {
            esc = true;
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn t9_chat_to_alice_via_menu() {
    let mut p = phone();
    // Menu -> Messages -> New message -> pick Alice (first contact)
    keys(&mut p, &[Key::Ok, Key::Num('1'), Key::Ok, Key::Ok]);
    run(&mut p, 1000);
    assert!(screen(&p).contains("Chat message"), "{}", screen(&p));
    // "Hi" by multi-tap: 44 (h, sentence-case H) pause 444 (i)
    keys(&mut p, &[Key::Num('4'), Key::Num('4')]);
    run(&mut p, 1200);
    keys(&mut p, &[Key::Num('4'), Key::Num('4'), Key::Num('4')]);
    run(&mut p, 1200);
    assert!(screen(&p).contains("Hi_"), "{}", screen(&p));
    keys(&mut p, &[Key::Ok]);
    run(&mut p, 6000);
    let s = screen(&p);
    assert!(s.contains("Alice · Chat"), "{}", s);
    assert!(s.contains("Read"), "{}", s);
    assert!(s.contains("Hey! How is life"), "{}", s);
    assert_eq!(p.core.unread_total(), 0, "thread open marks read");
}

#[test]
fn dial_from_home_and_hang_up() {
    let mut p = phone();
    typed(&mut p, "15550101");
    keys(&mut p, &[Key::Call]);
    run(&mut p, 4500);
    assert!(screen(&p).contains("Alice"), "{}", screen(&p));
    assert!(screen(&p).contains("00:0"), "{}", screen(&p));
    keys(&mut p, &[Key::End]);
    assert!(p.core.call.is_none());
    assert_eq!(p.depth(), 1);
    // IMEI code
    typed(&mut p, "*#06#");
    assert!(screen(&p).contains(crate::sim::IMEI));
}

#[test]
fn incoming_call_and_mms_flow() {
    let mut p = phone();
    p.core.simulate(1);
    run(&mut p, 500);
    assert!(screen(&p).contains("Incoming call"), "{}", screen(&p));
    keys(&mut p, &[Key::Call]);
    run(&mut p, 1500);
    assert!(screen(&p).contains("00:01"), "{}", screen(&p));
    keys(&mut p, &[Key::Back]);
    run(&mut p, 100);
    assert_eq!(p.depth(), 1);
    // Send an MMS with a picture to Bob via Contacts
    keys(&mut p, &[Key::Opt]); // Contacts
    keys(&mut p, &[Key::Down, Key::Down]); // Bob (sorted: + Add, Alice, Bob)
    keys(&mut p, &[Key::Ok, Key::Down, Key::Ok]); // Send message
    typed(&mut p, "pic");
    keys(&mut p, &[Key::Opt, Key::Num('2')]); // insert smiley
    assert!(screen(&p).contains("MMS"), "{}", screen(&p));
    keys(&mut p, &[Key::Ok]);
    run(&mut p, 10_000);
    let s = screen(&p);
    assert!(s.contains("██████"), "{}", s);
}

#[test]
fn settings_and_ussd() {
    let mut p = phone();
    typed(&mut p, "*100#");
    assert!(screen(&p).contains("Service code"), "{}", screen(&p));
    keys(&mut p, &[Key::Call]);
    run(&mut p, 1500);
    assert!(screen(&p).contains("Balance"), "{}", screen(&p));
    keys(&mut p, &[Key::Ok, Key::Ok, Key::Num('6')]); // Menu → Settings
    keys(&mut p, &[Key::Down, Key::Ok, Key::Num('2')]); // network mode → 4G only
    run(&mut p, 4000);
    assert!(screen(&p).contains("4G only"), "{}", screen(&p));
    assert!(p.render().status.starts_with("4G"));
    // calculator
    keys(&mut p, &[Key::End, Key::Ok, Key::Num('5')]);
    typed(&mut p, "12");
    keys(&mut p, &[Key::Left]);
    typed(&mut p, "3");
    keys(&mut p, &[Key::Up]);
    typed(&mut p, "4");
    keys(&mut p, &[Key::Ok]);
    assert!(screen(&p).contains("= 40"), "{}", screen(&p));
}

#[test]
fn data_survives_reboot() {
    use crate::store::{BlockStore, RamDisk, SECTOR};
    use std::rc::Rc;
    use std::cell::RefCell;
    // a disk shared between two "boots"
    struct Shared(Rc<RefCell<Vec<u8>>>);
    impl crate::store::BlockDev for Shared {
        fn sectors(&self) -> u64 { (self.0.borrow().len() / SECTOR) as u64 }
        fn read(&mut self, lba: u64, b: &mut [u8]) -> bool { RamDisk(self.0.borrow().clone()).read(lba, b) }
        fn write(&mut self, lba: u64, b: &[u8]) -> bool {
            let mut r = RamDisk(core::mem::take(&mut *self.0.borrow_mut()));
            let ok = r.write(lba, b);
            *self.0.borrow_mut() = r.0;
            ok
        }
    }
    let disk = Rc::new(RefCell::new(vec![0u8; 2048 * SECTOR]));
    NOW.with(|n| n.set(1));
    let mut p = Phone::new(Box::new(SimModem::new()), clock);
    p.set_storage(Box::new(BlockStore::new(Shared(disk.clone()))));
    p.boot();
    assert!(!p.restored);
    typed(&mut p, "1234");
    keys(&mut p, &[Key::Ok]);
    run(&mut p, 5000);
    p.core.contacts.push(crate::services::Contact { name: "Zoe".into(), number: "+15550177".into() });
    p.core.settings.alarm = Some((6, 30));
    p.core.settings.alarm_on = true;
    p.core.send_message("+15550101", "remember me", vec![], false);
    run(&mut p, 7000);
    p.core.dial("+15550102").unwrap();
    run(&mut p, 4000);
    p.core.hangup();
    run(&mut p, 2500);
    let before = p.core.snapshot();
    drop(p);

    let mut p2 = Phone::new(Box::new(SimModem::new()), clock);
    p2.set_storage(Box::new(BlockStore::new(Shared(disk))));
    p2.boot();
    assert!(p2.restored);
    assert_eq!(p2.core.snapshot(), before);
    assert!(p2.core.contacts.iter().any(|c| c.name == "Zoe"));
    assert!(p2.core.settings.alarm_on);
    let t = p2.core.thread("+15550101").unwrap().clone();
    assert!(t.msgs.iter().any(|m| m.text == "remember me"));
    assert!(t.msgs.iter().any(|m| m.incoming));
    assert_eq!(p2.core.log.len(), 1);
    // new messages don't reuse ids
    typed(&mut p2, "1234");
    keys(&mut p2, &[Key::Ok]);
    run(&mut p2, 5000);
    let id = p2.core.send_message("+15550101", "again", vec![], false);
    assert!(t.msgs.iter().all(|m| m.id != id));
}

fn goertzel(x: &[i16], f: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * f / crate::audio::RATE as f64;
    let (mut s1, mut s2) = (0.0, 0.0);
    for &v in x {
        let s = v as f64 + 2.0 * w.cos() * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    (s1 * s1 + s2 * s2 - 2.0 * w.cos() * s1 * s2).sqrt() / x.len() as f64
}

fn listen(p: &mut Phone, ms: usize) -> Vec<i16> {
    let mut v = vec![0i16; 48 * ms];
    p.audio(&mut v);
    v
}

#[test]
fn call_audio() {
    let mut p = phone();
    listen(&mut p, 600); // flush PIN key tones
    let quiet = listen(&mut p, 100);
    assert!(quiet.iter().all(|&s| s == 0), "idle is silent");
    p.core.dial("+15550101").unwrap();
    run(&mut p, 1500); // alerting
    let rb = listen(&mut p, 200);
    assert!(goertzel(&rb, 440.0) > 500.0 && goertzel(&rb, 480.0) > 500.0, "ringback 440+480");
    assert!(goertzel(&rb, 1000.0) < 50.0);
    run(&mut p, 3000); // answered
    let v = listen(&mut p, 1000);
    let rms = (v.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / v.len() as f64).sqrt();
    assert!(rms > 500.0, "far-end voice rms {}", rms);
    p.key(Key::Num('5'));
    let d = listen(&mut p, 100);
    assert!(goertzel(&d, 770.0) > 300.0 && goertzel(&d, 1336.0) > 300.0, "DTMF 5");
    p.key(Key::End);
    run(&mut p, 1000);
    p.core.simulate(1);
    run(&mut p, 500);
    let ring = listen(&mut p, 1000);
    assert!(goertzel(&ring, 659.3) + goertzel(&ring, 784.0) > 300.0, "ringtone");
}

#[test]
fn raster_layout_and_diff() {
    let mut p = phone();
    let f = p.render();
    let cells = raster::cells(&f);
    assert_eq!(cells.len(), raster::COLS * raster::LINES);
    let mut img = raster::Image::new(1080, 2280);
    let mut r = raster::Raster::new(1080, 2280);
    assert_eq!(r.scale, 5);
    assert_eq!(r.draw(&f, &mut img), cells.len());
    assert_eq!(r.draw(&f, &mut img), 0, "nothing changed");
    p.key(Key::Ok); // open the menu
    let n = r.draw(&p.render(), &mut img);
    assert!(n > 0 && n < cells.len());
    if let Ok(dir) = std::env::var("TQ_SCREENSHOT_DIR") {
        // menu → messages → thread with Alice after a chat round-trip
        keys(&mut p, &[Key::Back]);
        p.core.send_message("+15550101", "Hello from Tranquility", vec![], false);
        run(&mut p, 7000);
        keys(&mut p, &[Key::Left, Key::Down, Key::Ok]);
        let mut img = raster::Image::new(720, 1440);
        raster::Raster::new(720, 1440).draw(&p.render(), &mut img);
        std::fs::write(format!("{}/thread.ppm", dir), img.ppm()).unwrap();
    }
}

#[test]
fn create_group_from_ui() {
    let mut p = phone();
    // Messages → New group → pick Alice and Carol → Next → name "Hike" → Create
    keys(&mut p, &[Key::Left, Key::Down, Key::Ok]);
    assert!(screen(&p).contains("New group (0)"), "{}", screen(&p));
    keys(&mut p, &[Key::Ok, Key::Down, Key::Down, Key::Ok]); // Alice, (Bob skipped), Carol
    run(&mut p, 1000);
    assert!(screen(&p).contains("New group (2)"), "{}", screen(&p));
    keys(&mut p, &[Key::Opt]);
    typed(&mut p, "Hike");
    keys(&mut p, &[Key::Ok]);
    assert!(screen(&p).contains("Group chat"), "{}", screen(&p));
    typed(&mut p, "saturday?");
    keys(&mut p, &[Key::Ok]);
    run(&mut p, 8000);
    let s = screen(&p);
    assert!(s.contains("Hike · Chat"), "{}", s);
    assert!(s.contains("Alice") && s.contains("Carol"), "senders shown: {}", s);
}

fn title(p: &Phone) -> String {
    p.render().title.trim().to_string()
}

#[test]
fn nav_back_returns_to_menu_from_every_app() {
    let mut p = phone();
    for item in 1..=8u8 {
        keys(&mut p, &[Key::Ok]);
        assert_eq!(title(&p), "Menu");
        let d = p.depth();
        keys(&mut p, &[Key::Num((b'0' + item) as char)]);
        assert!(p.depth() > d, "item {} opened nothing", item);
        let opened = title(&p);
        // up to 6 Backs (text fields eat some); must land on the menu
        for _ in 0..6 {
            if title(&p) == "Menu" {
                break;
            }
            keys(&mut p, &[Key::Back]);
        }
        assert_eq!(title(&p), "Menu", "Back from item {} ({}) didn't return to the menu", item, opened);
        keys(&mut p, &[Key::Back]);
        assert_eq!(p.depth(), 1, "Back from menu goes home");
    }
}

#[test]
fn nav_send_from_thread_then_back() {
    let mut p = phone();
    p.core.send_message("+15550101", "hi", vec![], false);
    run(&mut p, 6000);
    keys(&mut p, &[Key::Left]); // Messages
    let threads_depth = p.depth();
    keys(&mut p, &[Key::Down, Key::Down, Key::Ok]); // newest thread
    assert!(title(&p).starts_with("Alice"), "{}", title(&p));
    keys(&mut p, &[Key::Ok]); // write
    typed(&mut p, "again");
    keys(&mut p, &[Key::Ok]); // send
    assert!(title(&p).starts_with("Alice"));
    keys(&mut p, &[Key::Back]);
    assert_eq!(p.depth(), threads_depth, "one Back from the thread returns to the list");
    assert_eq!(title(&p), "Messages");
}

#[test]
fn nav_exit_key_leaves_text_screens() {
    let mut p = phone();
    keys(&mut p, &[Key::Left, Key::Ok, Key::Ok]); // Messages → New message → Alice → Compose
    typed(&mut p, "draft text");
    assert!(title(&p).starts_with("To "), "{}", title(&p));
    keys(&mut p, &[Key::Exit]);
    assert_eq!(title(&p), "Messages", "Exit leaves compose even with text");
}

#[test]
fn deletes_ask_first() {
    let mut p = phone();
    p.core.send_message("+15550101", "keep me", vec![], false);
    run(&mut p, 500);
    keys(&mut p, &[Key::Left, Key::Down, Key::Down, Key::Opt]);
    assert_eq!(title(&p), "Delete conversation?");
    keys(&mut p, &[Key::Back]);
    assert_eq!(p.core.threads.len(), 1, "Back cancels");
    keys(&mut p, &[Key::Opt, Key::Down, Key::Ok]); // "Cancel"
    assert_eq!(p.core.threads.len(), 1);
    keys(&mut p, &[Key::Opt, Key::Ok]); // "Delete"
    assert!(p.core.threads.iter().all(|t| t.msgs.is_empty()) || p.core.threads.is_empty());
    let n = p.core.contacts.len();
    keys(&mut p, &[Key::End, Key::Opt, Key::Down, Key::Ok]); // Contacts → Alice
    keys(&mut p, &[Key::Num('5')]); // Delete
    assert_eq!(title(&p), "Delete contact?");
    keys(&mut p, &[Key::Num('2')]); // Cancel
    assert_eq!(p.core.contacts.len(), n);
    keys(&mut p, &[Key::Num('5'), Key::Num('1')]);
    assert_eq!(p.core.contacts.len(), n - 1);
    assert_eq!(title(&p), "Contacts");
}


#[test]
fn languages() {
    let mut p = phone();
    // Menu → Settings → Language → Français
    keys(&mut p, &[Key::Ok, Key::Num('6'), Key::Ok, Key::Num('2')]);
    assert_eq!(title(&p), "Paramètres");
    keys(&mut p, &[Key::End]);
    let s = screen(&p);
    assert!(s.contains("sam. 21 nov."), "French date: {}", s);
    assert_eq!(p.render().soft[0], "Noms");
    // T9 in French: key 3 pressed twice → é; # # → upper case → É
    keys(&mut p, &[Key::Left, Key::Ok, Key::Ok]); // Messages → New → Alice
    assert!(title(&p).starts_with("À "), "{}", title(&p));
    keys(&mut p, &[Key::Num('#'), Key::Num('3'), Key::Num('3'), Key::Num('3'), Key::Num('3')]);
    run(&mut p, 1200);
    assert!(screen(&p).contains("é"), "{}", screen(&p));
    keys(&mut p, &[Key::Exit, Key::Exit, Key::End]);
    // every language renders every main screen without panicking, and soft keys fit
    for (i, l) in i18n::Lang::ALL.iter().enumerate() {
        p.core.settings.language = *l;
        for item in 1..=8u8 {
            keys(&mut p, &[Key::Ok, Key::Num((b'0' + item) as char)]);
            let f = p.render();
            for sk in &f.soft {
                assert!(crate::util::char_len(sk) <= 8, "{:?}: {:?}", l, sk);
            }
            keys(&mut p, &[Key::End]);
        }
        let _ = i;
    }
    // the language is saved with the user's data
    p.core.settings.language = i18n::Lang::De;
    let snap = p.core.snapshot();
    let mut q = Phone::new(Box::new(SimModem::new()), clock);
    assert!(q.core.restore(&snap));
    assert_eq!(q.core.settings.language, i18n::Lang::De);
}
