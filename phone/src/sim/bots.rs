//! Simulated subscribers on the network.
use crate::prelude::*;
use crate::util::same_number;

pub struct Bot {
    pub number: &'static str,
    pub name: &'static str,
    pub rcs: bool,
    pub answers_calls: bool,
    pub hangs_up_after: Option<u64>,
    pub replies: bool,
}

pub const BOTS: &[Bot] = &[
    Bot { number: "+15550101", name: "Alice", rcs: true, answers_calls: true, hangs_up_after: None, replies: true },
    Bot { number: "+15550102", name: "Bob", rcs: false, answers_calls: true, hangs_up_after: Some(15_000), replies: true },
    Bot { number: "+15550103", name: "Carol", rcs: true, answers_calls: false, hangs_up_after: None, replies: true },
    Bot { number: "+15550104", name: "Dave", rcs: false, answers_calls: true, hangs_up_after: Some(5_000), replies: false },
];

pub fn find(number: &str) -> Option<&'static Bot> {
    BOTS.iter().find(|b| same_number(b.number, number))
}

pub fn reply(b: &Bot, text: &str, rcs: bool) -> Option<String> {
    if !b.replies {
        return None;
    }
    let t = text.to_ascii_lowercase();
    let r = if t.contains("hello") || t.starts_with("hi") || t.starts_with("hey") {
        match b.name {
            "Alice" => "Hey! How is life on the dumbphone?".to_string(),
            "Bob" => "Hello hello. Bob here.".to_string(),
            _ => format!("Hi, {} here!", b.name),
        }
    } else if t.contains('?') {
        format!("Good question. {} will think about it.", b.name)
    } else if t.contains("call") {
        "Sure, call me any time.".to_string()
    } else {
        let short: String = text.chars().take(60).collect();
        format!("{} got: \"{}\"", b.name, short)
    };
    Some(if rcs { format!("{} \u{1F44D}", r) } else { r })
}
