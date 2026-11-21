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
    keys(&mut p, &[Key::Ok, Key::Num('2')]); // network mode → 4G only
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
