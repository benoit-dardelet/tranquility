//! End-to-end tests: the full phone stack against the simulated network.
use crate::prelude::*;
use crate::services::rcs::RegState;
use crate::services::*;
use crate::sim::SimModem;
use std::cell::Cell;

thread_local!(static NOW: Cell<u64> = const { Cell::new(0) });

fn clock() -> u64 {
    NOW.with(|c| c.get())
}

fn run(c: &mut Core, ms: u64) {
    for _ in 0..ms / 10 {
        NOW.with(|n| n.set(n.get() + 10));
        c.step(clock());
    }
}

fn booted() -> Core {
    NOW.with(|n| n.set(1));
    let mut c = Core::new(Box::new(SimModem::new()), clock);
    c.boot();
    assert!(c.sim_locked);
    assert!(!c.enter_pin("0000"));
    assert_eq!(c.pin_tries, 2);
    assert!(c.enter_pin(crate::sim::SIM_PIN));
    run(&mut c, 3000);
    assert!(c.net.registered, "not registered");
    run(&mut c, 2000);
    c
}

fn thread<'a>(c: &'a Core, n: &str) -> &'a Thread {
    c.threads.iter().find(|t| crate::util::same_number(&t.number, n)).expect("thread")
}

#[test]
fn boot_registers_5g_and_rcs() {
    let c = booted();
    assert_eq!(c.info.imei, crate::sim::IMEI);
    assert_eq!(c.info.number, crate::sim::OWN_NUMBER);
    assert_eq!(c.net.rat(), "5G");
    assert_eq!(c.net.operator, "Tranquil Mobile");
    assert!(c.net.pdp_up);
    assert_eq!(c.rcs.state, RegState::Registered);
    // network clock: 12:00 local (UTC-5)
    let d = crate::util::civil_from_unix(c.local());
    assert_eq!((d.year, d.month, d.day, d.hour), (2026, 11, 21, 12));
}

#[test]
fn sms_roundtrip_with_delivery_report() {
    let mut c = booted();
    c.set_rcs(false);
    c.send_message("+15550102", "hello Bob", vec![], false);
    run(&mut c, 6000);
    let t = thread(&c, "+15550102");
    assert_eq!(t.msgs[0].transport, Transport::Sms);
    assert_eq!(t.msgs[0].status, Status::Delivered);
    assert!(t.msgs.len() >= 2, "{:?}", t.msgs);
    assert!(t.msgs[1].incoming);
    assert_eq!(t.msgs[1].text, "Hello hello. Bob here.");
}

#[test]
fn long_and_unicode_sms() {
    let mut c = booted();
    c.set_rcs(false);
    let long: String = core::iter::repeat("abcdefghij").take(35).collect();
    c.send_message("+15550100", &long, vec![], false);
    c.send_message("+15550100", "Grüße 日本 🙂", vec![], false);
    run(&mut c, 5000);
    let t = thread(&c, "+15550100");
    let incoming: Vec<&str> = t.msgs.iter().filter(|m| m.incoming).map(|m| m.text.as_str()).collect();
    assert!(incoming.contains(&long.as_str()), "{:?}", incoming);
    assert!(incoming.contains(&"Grüße 日本 🙂"));
    c.simulate(7);
    run(&mut c, 2000);
    let t = &thread(&c, "+15550103").msgs;
    assert_eq!(t.len(), 1, "{:?}", t);
    assert!(t[0].text.contains("reassembles"), "{:?}", t);
}

#[test]
fn rcs_chat_receipts_and_typing() {
    let mut c = booted();
    let id = c.send_message("+15550101", "hello Alice", vec![], false);
    run(&mut c, 1000);
    assert_eq!(c.peer_rcs("+15550101"), Some(true));
    let m = thread(&c, "+15550101").msgs.iter().find(|m| m.id == id).unwrap().clone();
    assert_eq!(m.transport, Transport::Rcs);
    assert!(m.status >= Status::Sent);
    run(&mut c, 1600);
    assert!(thread(&c, "+15550101").typing_until > c.now, "typing indicator");
    run(&mut c, 3000);
    let t = thread(&c, "+15550101");
    assert_eq!(t.msgs[0].status, Status::Read);
    let reply = t.msgs.iter().find(|m| m.incoming).expect("reply");
    assert_eq!(reply.transport, Transport::Rcs);
    assert!(reply.text.starts_with("Hey!"));
    assert!(reply.unread && reply.wants_display);
    assert_eq!(t.typing_until, 0);
    c.mark_read("+15550101");
    run(&mut c, 500);
    assert_eq!(c.unread_total(), 0);
}

#[test]
fn rcs_falls_back_to_sms_for_non_rcs_peer() {
    let mut c = booted();
    let id = c.send_message("+15550102", "hi Bob", vec![], false);
    run(&mut c, 6000);
    let t = thread(&c, "+15550102");
    let m = t.msgs.iter().find(|m| m.id == id).unwrap();
    assert_eq!(m.transport, Transport::Sms);
    assert_eq!(c.peer_rcs("+15550102"), Some(false));
    assert!(t.msgs.iter().any(|m| m.incoming));
}

#[test]
fn incoming_rcs_when_offline_arrives_as_sms() {
    let mut c = booted();
    c.set_rcs(false);
    c.simulate(3);
    run(&mut c, 4000);
    let t = thread(&c, "+15550101");
    assert_eq!(t.msgs[0].transport, Transport::Sms);
}

#[test]
fn mms_send_and_receive() {
    let mut c = booted();
    let pic = crate::mms::Part { content_type: "image/bmp".into(), name: "heart.bmp".into(), data: crate::bmp::encode(&crate::bmp::HEART) };
    let id = c.send_message("+15550102", "look at this", vec![pic], false);
    run(&mut c, 10_000);
    let t = thread(&c, "+15550102");
    let sent = t.msgs.iter().find(|m| m.id == id).unwrap();
    assert_eq!(sent.transport, Transport::Mms);
    assert_eq!(sent.status, Status::Sent);
    let got = t.msgs.iter().find(|m| m.incoming).expect("bot MMS reply");
    assert_eq!(got.transport, Transport::Mms);
    assert_eq!(got.attachments.len(), 1);
    assert!(crate::bmp::render(&got.attachments[0].data).is_some());
    assert_eq!(c.mms_pending(), 0);
}

#[test]
fn mms_to_self_roundtrip() {
    let mut c = booted();
    c.set_rcs(false);
    let vcard = crate::mms::Part { content_type: "text/x-vcard".into(), name: "alice.vcf".into(), data: b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Alice\r\nTEL:+15550101\r\nEND:VCARD\r\n".to_vec() };
    c.send_message("+15550100", "me to me", vec![vcard.clone()], false);
    run(&mut c, 6000);
    let got = thread(&c, "+15550100").msgs.iter().find(|m| m.incoming).expect("self MMS");
    assert_eq!(got.text, "me to me");
    assert_eq!(got.attachments, vec![vcard]);
}

#[test]
fn voice_calls() {
    let mut c = booted();
    c.dial("+15550101").unwrap();
    run(&mut c, 1200);
    assert_eq!(c.call.as_ref().unwrap().state, CallState::Alerting);
    run(&mut c, 3000);
    assert_eq!(c.call.as_ref().unwrap().state, CallState::Active);
    c.dtmf('5');
    run(&mut c, 2000);
    c.hangup();
    assert!(c.call.is_none());
    assert_eq!(c.log.last().unwrap().kind, LogKind::Dialed);
    assert!(c.log.last().unwrap().secs >= 1);

    // busy
    c.dial("+15550199").unwrap();
    run(&mut c, 2000);
    assert!(c.call.is_none());
    assert!(c.events.iter().any(|e| matches!(e, Event::CallEnded { reason, .. } if reason == "Busy")));

    // incoming, answered
    c.events.clear();
    c.simulate(1);
    run(&mut c, 500);
    assert_eq!(c.events.front(), Some(&Event::IncomingCall("+15550102".into())));
    c.answer();
    run(&mut c, 1000);
    assert_eq!(c.call.as_ref().unwrap().state, CallState::Active);
    c.hangup();
    assert_eq!(c.log.last().unwrap().kind, LogKind::Received);

    // incoming, missed
    c.simulate(1);
    run(&mut c, 26_000);
    assert!(c.call.is_none());
    assert_eq!(c.log.last().unwrap().kind, LogKind::Missed);
    assert_eq!(c.missed, 1);

    // emergency works in airplane mode
    c.set_airplane(true);
    assert!(c.dial("+15550101").is_err());
    c.dial("112").unwrap();
    c.hangup();
}

#[test]
fn ussd_and_cell_broadcast() {
    let mut c = booted();
    c.ussd("*100#");
    run(&mut c, 1000);
    assert!(c.events.iter().any(|e| matches!(e, Event::Ussd(t) if t.contains("Balance"))));
    c.simulate(5);
    run(&mut c, 1000);
    assert!(c.events.iter().any(|e| matches!(e, Event::Alert { title, .. } if title == "Presidential alert")));
}

#[test]
fn network_mode_airplane_and_signal_loss() {
    let mut c = booted();
    c.set_net_mode(NetMode::Lte);
    run(&mut c, 3000);
    assert_eq!(c.net.rat(), "4G");
    run(&mut c, 3000);
    assert_eq!(c.rcs.state, RegState::Registered, "RCS re-registers after RAT change");
    c.set_airplane(true);
    run(&mut c, 500);
    assert!(!c.net.registered);
    let id = c.send_message("+15550102", "offline", vec![], false);
    assert_eq!(c.find_msg(id).unwrap().status, Status::Failed);
    c.set_airplane(false);
    run(&mut c, 3000);
    assert!(c.net.registered);
    c.simulate(6);
    run(&mut c, 500);
    assert!(!c.net.registered);
    run(&mut c, 9000);
    assert!(c.net.registered);
}

#[test]
fn rcs_file_transfer_both_ways() {
    let mut c = booted();
    run(&mut c, 500);
    // learn Alice's capabilities first so the picture goes by chat
    c.rcs_query_caps("+15550101");
    run(&mut c, 500);
    let pic = crate::mms::Part { content_type: "image/bmp".into(), name: "smiley.bmp".into(), data: crate::bmp::encode(&crate::bmp::SMILEY) };
    let id = c.send_message("+15550101", "", vec![pic], false);
    run(&mut c, 8000);
    let t = thread(&c, "+15550101");
    let sent = t.msgs.iter().find(|m| m.id == id).unwrap();
    assert_eq!(sent.transport, Transport::Rcs, "{:?}", sent);
    assert_eq!(sent.status, Status::Read);
    let file = t.msgs.iter().find(|m| m.incoming && !m.attachments.is_empty()).expect("Alice sends a file back");
    assert_eq!(file.transport, Transport::Rcs);
    assert_eq!(file.attachments[0].name, "heart.bmp");
    assert!(crate::bmp::render(&file.attachments[0].data).is_some());
    assert!(t.msgs.iter().any(|m| m.incoming && m.text.contains("nice picture")));
    assert_eq!(c.http_pending(), 0);
}

#[test]
fn rcs_group_chat() {
    let mut c = booted();
    let key = c.create_group("Hike", &["+15550101".into(), "+15550103".into()], None);
    run(&mut c, 1000);
    let id = c.send_message(&key, "hello group", vec![], false);
    run(&mut c, 8000);
    let t = c.thread(&key).unwrap();
    assert!(t.is_group());
    assert_eq!(t.members, vec!["+15550101".to_string(), "+15550103".to_string()]);
    let m = t.msgs.iter().find(|m| m.id == id).unwrap();
    assert_eq!(m.transport, Transport::Rcs);
    assert!(m.status >= Status::Delivered, "{:?}", m.status);
    let from: Vec<&str> = t.msgs.iter().filter(|m| m.incoming).map(|m| m.from.as_str()).collect();
    assert!(from.contains(&"+15550101") && from.contains(&"+15550103"), "{:?}", t.msgs);
    // group started by someone else
    c.simulate(8);
    run(&mut c, 6000);
    let g = c.threads.iter().find(|t| t.group_name == "Weekend plans").expect("incoming group");
    assert_eq!(g.msgs.len(), 3);
    assert!(g.msgs[2].attachments.len() == 1, "file in group: {:?}", g.msgs[2]);
}

#[test]
fn group_mms_when_someone_cannot_chat() {
    let mut c = booted();
    // Bob has no RCS: the group falls back to group MMS, and Bob replies to all.
    let key = c.create_group("", &["+15550101".into(), "+15550102".into()], None);
    run(&mut c, 1000);
    let id = c.send_message(&key, "dinner tonight?", vec![], false);
    run(&mut c, 10_000);
    let t = c.thread(&key).unwrap();
    let m = t.msgs.iter().find(|m| m.id == id).unwrap();
    assert_eq!(m.transport, Transport::Mms);
    assert_eq!(m.status, Status::Sent);
    let reply = t.msgs.iter().find(|m| m.incoming).expect("reply-all lands in the same group thread");
    assert!(t.members.contains(&reply.from), "{:?}", reply.from);
    assert_eq!(c.threads.iter().filter(|t| t.is_group()).count(), 1);
}
