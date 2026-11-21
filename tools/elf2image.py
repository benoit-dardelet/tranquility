#!/usr/bin/env python3
"""Turn the kernel ELF (PIE, linked at 0) into a flat arm64 `Image` and
`Image.gz`, checking the header and that only RELATIVE relocations remain."""
import gzip, struct, sys

def main(elf_path, out):
    d = open(elf_path, "rb").read()
    assert d[:4] == b"\x7fELF" and d[4] == 2, "not ELF64"
    e_phoff, e_shoff = struct.unpack_from("<QQ", d, 0x20)
    e_phentsize, e_phnum, e_shentsize, e_shnum, e_shstrndx = struct.unpack_from("<HHHHH", d, 0x36)
    image = bytearray()
    for i in range(e_phnum):
        p_type, _, p_off, p_vaddr, _, p_filesz, p_memsz, _ = struct.unpack_from("<IIQQQQQQ", d, e_phoff + i * e_phentsize)
        if p_type != 1 or p_filesz == 0:
            continue
        if len(image) < p_vaddr:
            image += bytes(p_vaddr - len(image))
        image[p_vaddr:p_vaddr + p_filesz] = d[p_off:p_off + p_filesz]
    # sections: verify relocations
    sh = [struct.unpack_from("<IIQQQQIIQQ", d, e_shoff + i * e_shentsize) for i in range(e_shnum)]
    strtab = sh[e_shstrndx]
    name = lambda off: d[strtab[4] + off:d.index(b"\0", strtab[4] + off)].decode()
    nrel = 0
    for s in sh:
        if s[1] == 4:  # SHT_RELA
            for o in range(s[4], s[4] + s[5], 24):
                _, info, _ = struct.unpack_from("<QQq", d, o)
                assert info & 0xFFFFFFFF == 1027, f"non-RELATIVE relocation {info & 0xFFFFFFFF} in {name(s[0])}"
                nrel += 1
    magic = image[56:60]
    assert magic == b"ARM\x64", magic
    text_offset, image_size, flags = struct.unpack_from("<QQQ", image, 8)
    assert image_size >= len(image), (image_size, len(image))
    open(out, "wb").write(image)
    with open(out + ".gz", "wb") as f:
        f.write(gzip.compress(bytes(image), compresslevel=9, mtime=0))
    print(f"{out}: {len(image)} bytes (effective {image_size}), {nrel} relocations, text_offset {text_offset:#x}, flags {flags:#x}")

if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
