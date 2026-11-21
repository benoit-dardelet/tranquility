// Flattened device tree (DTB) writer — DTSpec v0.4, format version 17.

export class DtNode {
  constructor(name) {
    this.name = name;
    this.props = [];
    this.children = [];
  }
  /** value: number[] (u32 cells) | string | string[] | Uint8Array | true (empty) */
  prop(name, value) {
    this.props.push([name, value]);
    return this;
  }
  child(name) {
    const n = new DtNode(name);
    this.children.push(n);
    return n;
  }
}

const enc = new TextEncoder();

function encodeValue(v) {
  if (v === true || v === undefined) return new Uint8Array(0);
  if (v instanceof Uint8Array) return v;
  if (typeof v === "string") return enc.encode(v + "\0");
  if (Array.isArray(v) && v.every((x) => typeof x === "string")) return enc.encode(v.map((s) => s + "\0").join(""));
  if (Array.isArray(v)) {
    const b = new Uint8Array(v.length * 4);
    const dv = new DataView(b.buffer);
    v.forEach((x, i) => dv.setUint32(i * 4, x >>> 0));
    return b;
  }
  throw new Error("unsupported property value " + v);
}

/** Split a 64-bit address/size into two u32 cells. */
export const cells64 = (x) => [Math.floor(x / 2 ** 32) >>> 0, x >>> 0];

export function buildDtb(root, { memReserve = [], bootCpuid = 0 } = {}) {
  const struct = [];
  const strings = [];
  const strOff = new Map();
  const u32 = (x) => struct.push(x >>> 0);
  const bytesPad = (b) => {
    const padded = new Uint8Array(Math.ceil(b.length / 4) * 4);
    padded.set(b);
    const dv = new DataView(padded.buffer);
    for (let i = 0; i < padded.length; i += 4) u32(dv.getUint32(i));
  };
  const nameOff = (n) => {
    if (!strOff.has(n)) {
      strOff.set(n, strings.reduce((a, s) => a + s.length, 0));
      strings.push(enc.encode(n + "\0"));
    }
    return strOff.get(n);
  };
  const walk = (node) => {
    u32(1); // FDT_BEGIN_NODE
    bytesPad(enc.encode(node.name + "\0"));
    for (const [n, v] of node.props) {
      const val = encodeValue(v);
      u32(3); // FDT_PROP
      u32(val.length);
      u32(nameOff(n));
      bytesPad(val);
    }
    node.children.forEach(walk);
    u32(2); // FDT_END_NODE
  };
  walk(root);
  u32(9); // FDT_END

  const rsv = new Uint8Array((memReserve.length + 1) * 16);
  const rdv = new DataView(rsv.buffer);
  memReserve.forEach(([a, s], i) => {
    rdv.setBigUint64(i * 16, BigInt(a));
    rdv.setBigUint64(i * 16 + 8, BigInt(s));
  });
  const strBytes = strings.reduce((a, s) => a + s.length, 0);
  const offRsv = 40;
  const offStruct = offRsv + rsv.length;
  const offStrings = offStruct + struct.length * 4;
  const total = offStrings + strBytes;
  const out = new Uint8Array(Math.ceil(total / 4) * 4);
  const dv = new DataView(out.buffer);
  [0xd00dfeed, out.length, offStruct, offStrings, offRsv, 17, 16, bootCpuid, strBytes, struct.length * 4].forEach((v, i) => dv.setUint32(i * 4, v));
  out.set(rsv, offRsv);
  struct.forEach((v, i) => dv.setUint32(offStruct + i * 4, v));
  let p = offStrings;
  for (const s of strings) {
    out.set(s, p);
    p += s.length;
  }
  return out;
}
