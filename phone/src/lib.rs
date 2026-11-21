//! Tranquility phone stack.
//!
//! Everything above the kernel lives here: protocol codecs (GSM 03.38,
//! TS 23.040 SMS, TS 23.041 cell broadcast, WSP / OMA MMS encapsulation,
//! HTTP/1.1, SIP, CPIM, IMDN, isComposing), the AT-command modem driver,
//! the phone services (telephony, messaging, MMS, RCS) and the dumbphone UI.
//!
//! The crate is `no_std` + `alloc` so the kernel can link it directly, and
//! it builds on the host for `cargo test`.
#![cfg_attr(not(test), no_std)]

extern crate alloc;

pub(crate) mod prelude {
    pub use alloc::boxed::Box;
    pub use alloc::collections::{BTreeMap, VecDeque};
    pub use alloc::format;
    pub use alloc::string::{String, ToString};
    pub use alloc::vec;
    pub use alloc::vec::Vec;
}

pub mod at;
pub mod bmp;
pub mod cb;
pub mod cpim;
pub mod gsm7;
pub mod md5;
pub mod mms;
pub mod msg;
pub mod services;
pub mod sim;
pub mod sip;
pub mod sms;
pub mod ui;
pub mod util;
pub mod wsp;

#[cfg(test)]
mod tests;
