//! virtio-snd driver: one PCM output stream, S16 48 kHz, fed with a ring
//! of periods that are refilled from the phone's audio mixer as the
//! device consumes them.
use crate::virtio::{dma, Mmio, Queue};
use alloc::vec::Vec;

const PCM_INFO: u32 = 0x0100;
const PCM_SET_PARAMS: u32 = 0x0101;
const PCM_PREPARE: u32 = 0x0102;
const PCM_START: u32 = 0x0104;
const S_OK: u32 = 0x8000;
const FMT_S16: u8 = 5;
const RATE_48000: u8 = 7;
const PERIOD_FRAMES: usize = 960; // 20 ms
const PERIODS: usize = 6;

pub struct VirtioSnd {
    m: Mmio,
    tx: Queue,
    stream: u32,
    channels: usize,
    bufs: Vec<(*mut u8, *mut u8, *mut u8)>, // xfer hdr, pcm, status
    by_head: Vec<(u16, usize)>,
    idle: Vec<usize>,
    scratch: Vec<i16>,
}

fn ctl(m: &Mmio, q: &mut Queue, req: &[u8], resp_len: usize) -> Option<Vec<u8>> {
    let rq = dma(req.len());
    let rs = dma(resp_len);
    unsafe { core::ptr::copy_nonoverlapping(req.as_ptr(), rq, req.len()) };
    q.submit(&[(rq as u64, req.len() as u32, false), (rs as u64, resp_len as u32, true)])?;
    m.notify(q);
    let start = crate::timer::now_ms();
    while q.pop().is_none() {
        if crate::timer::now_ms() - start > 1000 {
            return None;
        }
    }
    let out = unsafe { core::slice::from_raw_parts(rs, resp_len).to_vec() };
    (u32::from_le_bytes(out[0..4].try_into().ok()?) == S_OK).then_some(out)
}

fn le(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

impl VirtioSnd {
    pub fn new(m: Mmio) -> Option<VirtioSnd> {
        if !m.begin() {
            return None;
        }
        let streams = m.cfg32(4);
        let mut ctlq = m.queue(0, 16)?;
        let _evq = m.queue(1, 8)?;
        let tx = m.queue(2, 32)?;
        let _rxq = m.queue(3, 8)?;
        m.finish();
        // find an output stream supporting S16 @ 48 kHz
        let info = ctl(&m, &mut ctlq, &le(&[PCM_INFO, 0, streams, 32]), 4 + 32 * streams as usize)?;
        let mut pick = None;
        for i in 0..streams as usize {
            let e = &info[4 + i * 32..4 + (i + 1) * 32];
            let formats = u64::from_le_bytes(e[8..16].try_into().ok()?);
            let rates = u64::from_le_bytes(e[16..24].try_into().ok()?);
            let (dir, cmin, cmax) = (e[24], e[25], e[26]);
            if dir == 0 && formats & (1 << FMT_S16) != 0 && rates & (1 << RATE_48000) != 0 {
                pick = Some((i as u32, cmin.max(1).min(cmax.max(1)).min(2) as usize));
                break;
            }
        }
        let (stream, channels) = pick?;
        let period = (PERIOD_FRAMES * channels * 2) as u32;
        let mut p = le(&[PCM_SET_PARAMS, stream, period * PERIODS as u32, period, 0]);
        p.extend_from_slice(&[channels as u8, FMT_S16, RATE_48000, 0]);
        ctl(&m, &mut ctlq, &p, 4)?;
        ctl(&m, &mut ctlq, &le(&[PCM_PREPARE, stream]), 4)?;
        let bufs = (0..PERIODS).map(|_| (dma(4), dma(period as usize), dma(8))).collect();
        let mut s = VirtioSnd { m, tx, stream, channels, bufs, by_head: Vec::new(), idle: (0..PERIODS).collect(), scratch: alloc::vec![0; PERIOD_FRAMES] };
        // prime silence, then start
        s.pump(&mut |b: &mut [i16]| b.fill(0));
        ctl(&s.m, &mut ctlq, &le(&[PCM_START, stream]), 4)?;
        Some(s)
    }

    /// Reclaim consumed periods and refill them from `fill` (mono samples).
    pub fn pump(&mut self, fill: &mut dyn FnMut(&mut [i16])) {
        while let Some((head, _)) = self.tx.pop() {
            if let Some(p) = self.by_head.iter().position(|&(h, _)| h == head) {
                let (_, i) = self.by_head.swap_remove(p);
                self.idle.push(i);
            }
        }
        let mut submitted = false;
        while let Some(i) = self.idle.pop() {
            fill(&mut self.scratch);
            let (hdr, pcm, st) = self.bufs[i];
            unsafe {
                (hdr as *mut u32).write_volatile(self.stream);
                let out = pcm as *mut i16;
                for (k, &v) in self.scratch.iter().enumerate() {
                    for c in 0..self.channels {
                        out.add(k * self.channels + c).write_volatile(v);
                    }
                }
            }
            let bytes = (PERIOD_FRAMES * self.channels * 2) as u32;
            match self.tx.submit(&[(hdr as u64, 4, false), (pcm as u64, bytes, false), (st as u64, 8, true)]) {
                Some(head) => {
                    self.by_head.push((head, i));
                    submitted = true;
                }
                None => {
                    self.idle.push(i);
                    break;
                }
            }
        }
        if submitted {
            self.m.notify(&self.tx);
        }
    }
}
