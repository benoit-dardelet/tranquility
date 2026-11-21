//! Flattened device tree (DTB) parser — just enough to discover the
//! platform: memory, reserved regions, interrupt controller, timer, UART,
//! framebuffer, virtio-mmio devices and QEMU fw_cfg.
use alloc::vec::Vec;

pub struct Node<'a> {
    pub name: &'a str,
    pub parent: Option<usize>,
    pub props: Vec<(&'a str, &'a [u8])>,
}

pub struct Fdt<'a> {
    pub nodes: Vec<Node<'a>>,
    pub total_size: usize,
}

fn be32(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

fn cstr(d: &[u8], o: usize) -> Option<&str> {
    let end = o + d.get(o..)?.iter().position(|&b| b == 0)?;
    core::str::from_utf8(&d[o..end]).ok()
}

impl<'a> Fdt<'a> {
    /// # Safety
    /// `ptr` must point at a readable DTB.
    pub unsafe fn from_ptr(ptr: usize) -> Option<Fdt<'static>> {
        if ptr == 0 || ptr % 4 != 0 {
            return None;
        }
        let hdr = core::slice::from_raw_parts(ptr as *const u8, 40);
        if be32(hdr, 0)? != 0xD00D_FEED {
            return None;
        }
        let size = be32(hdr, 4)? as usize;
        Fdt::parse(core::slice::from_raw_parts(ptr as *const u8, size))
    }

    pub fn parse(d: &'a [u8]) -> Option<Fdt<'a>> {
        if be32(d, 0)? != 0xD00D_FEED {
            return None;
        }
        let off_struct = be32(d, 8)? as usize;
        let off_strings = be32(d, 12)? as usize;
        let mut nodes: Vec<Node> = Vec::new();
        let mut stack: Vec<usize> = Vec::new();
        let mut p = off_struct;
        loop {
            match be32(d, p)? {
                1 => {
                    let name = cstr(d, p + 4)?;
                    p = (p + 4 + name.len() + 1 + 3) & !3;
                    nodes.push(Node { name, parent: stack.last().copied(), props: Vec::new() });
                    stack.push(nodes.len() - 1);
                }
                2 => {
                    stack.pop();
                    p += 4;
                }
                3 => {
                    let len = be32(d, p + 4)? as usize;
                    let nameoff = be32(d, p + 8)? as usize;
                    let val = d.get(p + 12..p + 12 + len)?;
                    let name = cstr(d, off_strings + nameoff)?;
                    let cur = *stack.last()?;
                    nodes[cur].props.push((name, val));
                    p = (p + 12 + len + 3) & !3;
                }
                4 => p += 4,
                9 => break,
                _ => return None,
            }
        }
        Some(Fdt { nodes, total_size: d.len() })
    }

    pub fn prop(&self, n: usize, name: &str) -> Option<&'a [u8]> {
        self.nodes[n].props.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
    }

    pub fn u32(&self, n: usize, name: &str) -> Option<u32> {
        be32(self.prop(n, name)?, 0)
    }

    pub fn str(&self, n: usize, name: &str) -> Option<&'a str> {
        let v = self.prop(n, name)?;
        core::str::from_utf8(v.split(|&b| b == 0).next()?).ok()
    }

    pub fn compatible(&self, n: usize, c: &str) -> bool {
        self.prop(n, "compatible").is_some_and(|v| v.split(|&b| b == 0).any(|s| s == c.as_bytes()))
    }

    pub fn find_compatible(&self, cs: &[&str]) -> Vec<usize> {
        (0..self.nodes.len()).filter(|&n| cs.iter().any(|c| self.compatible(n, c))).collect()
    }

    pub fn by_path(&self, path: &str) -> Option<usize> {
        let mut cur = 0usize;
        for part in path.split('/').filter(|s| !s.is_empty()) {
            cur = (0..self.nodes.len()).find(|&i| {
                self.nodes[i].parent == Some(cur) && (self.nodes[i].name == part || self.nodes[i].name.split('@').next() == Some(part))
            })?;
        }
        Some(cur)
    }

    pub fn children(&self, n: usize) -> impl Iterator<Item = usize> + '_ {
        (0..self.nodes.len()).filter(move |&i| self.nodes[i].parent == Some(n))
    }

    fn cells(&self, n: usize) -> (usize, usize) {
        let p = self.nodes[n].parent.unwrap_or(0);
        let a = self.u32(p, "#address-cells").unwrap_or(2) as usize;
        let s = self.u32(p, "#size-cells").unwrap_or(1) as usize;
        (a, s)
    }

    /// `reg` entries as (address, size), using the parent's cell sizes.
    pub fn reg(&self, n: usize) -> Vec<(u64, u64)> {
        let (a, s) = self.cells(n);
        let v = match self.prop(n, "reg") {
            Some(v) => v,
            None => return Vec::new(),
        };
        let read = |o: usize, c: usize| (0..c).fold(0u64, |acc, i| acc << 32 | be32(v, o + i * 4).unwrap_or(0) as u64);
        let step = (a + s) * 4;
        if step == 0 {
            return Vec::new();
        }
        (0..v.len() / step).map(|i| (read(i * step, a), read(i * step + a * 4, s))).collect()
    }

    /// Interrupt specifiers (3-cell GIC style) as INTIDs.
    pub fn irqs(&self, n: usize) -> Vec<u32> {
        let v = match self.prop(n, "interrupts") {
            Some(v) => v,
            None => return Vec::new(),
        };
        v.chunks(12)
            .filter(|c| c.len() == 12)
            .map(|c| {
                let kind = be32(c, 0).unwrap_or(0);
                let num = be32(c, 4).unwrap_or(0);
                if kind == 1 {
                    num + 16
                } else {
                    num + 32
                }
            })
            .collect()
    }

}
