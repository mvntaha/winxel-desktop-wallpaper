use std::collections::BTreeMap;
use std::path::PathBuf;

const RT_ICON: u32 = 3;
const RT_GROUP_ICON: u32 = 14;
const RT_MANIFEST: u32 = 24;
const LANG_EN_US: u32 = 0x0409;
const ICON: &str = "assets/icon/winxel.ico";

fn main() {
    println!("cargo:rerun-if-changed=winxel.exe.manifest");
    println!("cargo:rerun-if-changed={ICON}");
    println!("cargo:rerun-if-changed=build.rs");
    let mut res: Vec<(u32, u32, Vec<u8>)> = Vec::new(); // (type, id, bytes)
    res.push((RT_MANIFEST, 1, std::fs::read("winxel.exe.manifest").expect("winxel.exe.manifest")));
    add_icon(&mut res, &std::fs::read(ICON).expect(ICON));
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("winxel_resources.o");
    std::fs::write(&out, rsrc_object(&res)).expect("write resource object");
    println!("cargo:rustc-link-arg-bins={}", out.display());
}

fn add_icon(res: &mut Vec<(u32, u32, Vec<u8>)>, ico: &[u8]) {
    let u16_at = |o: usize| u16::from_le_bytes([ico[o], ico[o + 1]]);
    let u32_at = |o: usize| u32::from_le_bytes(ico[o..o + 4].try_into().unwrap());
    assert!(u16_at(2) == 1, "{ICON} is not an icon file");
    let count = u16_at(4);
    let mut group = vec![0, 0, 1, 0];
    group.extend_from_slice(&count.to_le_bytes());
    for i in 0..count {
        let e = 6 + 16 * i as usize;
        let (size, offset) = (u32_at(e + 8) as usize, u32_at(e + 12) as usize);
        let id = i as u32 + 1;
        res.push((RT_ICON, id, ico[offset..offset + size].to_vec()));
        group.extend_from_slice(&ico[e..e + 12]); // size, colours, planes, bpp, bytes
        group.extend_from_slice(&(id as u16).to_le_bytes());
    }
    res.push((RT_GROUP_ICON, 1, group));
}

fn rsrc_object(res: &[(u32, u32, Vec<u8>)]) -> Vec<u8> {
    let mut types: BTreeMap<u32, BTreeMap<u32, &[u8]>> = BTreeMap::new();
    for (ty, id, data) in res {
        types.entry(*ty).or_default().insert(*id, data);
    }
    let leaves = res.len() as u32;
    let dir_size = |entries: u32| 16 + 8 * entries;

    // layout: root dir, type dirs, lang dirs (one per leaf), data entries, then data.
    let mut name_dir_at = Vec::new();
    let mut at = dir_size(types.len() as u32);
    for ids in types.values() {
        name_dir_at.push(at);
        at += dir_size(ids.len() as u32);
    }
    let lang_dirs_at = at;
    let entries_at = lang_dirs_at + leaves * dir_size(1);
    let mut data_at = entries_at + leaves * 16;

    let dir = |out: &mut Vec<u8>, entries: &[(u32, u32)]| {
        out.extend_from_slice(&[0u8; 12]); // Characteristics, TimeDateStamp, Major/MinorVersion
        out.extend_from_slice(&0u16.to_le_bytes()); // NumberOfNamedEntries
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes()); // NumberOfIdEntries
        for (name, target) in entries {
            out.extend_from_slice(&name.to_le_bytes());
            out.extend_from_slice(&target.to_le_bytes());
        }
    };
    const SUBDIR: u32 = 0x8000_0000;
    let mut rsrc = Vec::new();
    let root: Vec<_> = types.keys().zip(&name_dir_at).map(|(ty, at)| (*ty, SUBDIR | at)).collect();
    dir(&mut rsrc, &root);
    let mut leaf = 0;
    for ids in types.values() {
        let entries: Vec<_> = ids.keys().enumerate()
            .map(|(i, id)| (*id, SUBDIR | (lang_dirs_at + (leaf + i as u32) * dir_size(1))))
            .collect();
        dir(&mut rsrc, &entries);
        leaf += ids.len() as u32;
    }
    for i in 0..leaves {
        dir(&mut rsrc, &[(LANG_EN_US, entries_at + i * 16)]);
    }
    let mut relocs = Vec::new();
    let mut blobs = Vec::new();
    for data in types.values().flat_map(|ids| ids.values()) {
        relocs.push(rsrc.len() as u32);
        rsrc.extend_from_slice(&data_at.to_le_bytes());
        rsrc.extend_from_slice(&(data.len() as u32).to_le_bytes());
        rsrc.extend_from_slice(&[0u8; 8]);
        blobs.push((data_at, *data));
        data_at = (data_at + data.len() as u32).next_multiple_of(8);
    }
    for (at, data) in blobs {
        rsrc.resize(at as usize, 0);
        rsrc.extend_from_slice(data);
    }
    rsrc.resize(rsrc.len().next_multiple_of(8), 0);

    const HEADER: u32 = 20;
    const SECTION_HEADER: u32 = 40;
    let raw_ptr = HEADER + SECTION_HEADER;
    let reloc_ptr = raw_ptr + rsrc.len() as u32;
    let symtab_ptr = reloc_ptr + 10 * relocs.len() as u32;

    let mut o = Vec::new();
    // IMAGE_FILE_HEADER
    o.extend_from_slice(&0x8664u16.to_le_bytes()); // Machine: AMD64
    o.extend_from_slice(&1u16.to_le_bytes()); // NumberOfSections
    o.extend_from_slice(&0u32.to_le_bytes()); // TimeDateStamp
    o.extend_from_slice(&symtab_ptr.to_le_bytes()); // PointerToSymbolTable
    o.extend_from_slice(&1u32.to_le_bytes()); // NumberOfSymbols
    o.extend_from_slice(&0u16.to_le_bytes()); // SizeOfOptionalHeader
    o.extend_from_slice(&0u16.to_le_bytes()); // Characteristics
    // IMAGE_SECTION_HEADER
    o.extend_from_slice(b".rsrc\0\0\0");
    o.extend_from_slice(&0u32.to_le_bytes()); // VirtualSize
    o.extend_from_slice(&0u32.to_le_bytes()); // VirtualAddress
    o.extend_from_slice(&(rsrc.len() as u32).to_le_bytes()); // SizeOfRawData
    o.extend_from_slice(&raw_ptr.to_le_bytes()); // PointerToRawData
    o.extend_from_slice(&reloc_ptr.to_le_bytes()); // PointerToRelocations
    o.extend_from_slice(&0u32.to_le_bytes()); // PointerToLinenumbers
    o.extend_from_slice(&(relocs.len() as u16).to_le_bytes()); // NumberOfRelocations
    o.extend_from_slice(&0u16.to_le_bytes()); // NumberOfLinenumbers
    // INITIALIZED_DATA | ALIGN_4BYTES | MEM_READ
    o.extend_from_slice(&0x4030_0040u32.to_le_bytes());
    // section data
    o.extend_from_slice(&rsrc);
    // relocations: each data entry's OffsetToData += RVA(.rsrc) (IMAGE_REL_AMD64_ADDR32NB)
    for at in relocs {
        o.extend_from_slice(&at.to_le_bytes()); // VirtualAddress (offset in section)
        o.extend_from_slice(&0u32.to_le_bytes()); // SymbolTableIndex: the .rsrc section symbol
        o.extend_from_slice(&3u16.to_le_bytes()); // Type: ADDR32NB
    }
    // symbol 0: the section symbol for .rsrc
    o.extend_from_slice(b".rsrc\0\0\0");
    o.extend_from_slice(&0u32.to_le_bytes()); // Value
    o.extend_from_slice(&1i16.to_le_bytes()); // SectionNumber
    o.extend_from_slice(&0u16.to_le_bytes()); // Type
    o.push(3); // StorageClass: STATIC
    o.push(0); // NumberOfAuxSymbols
    // string table (empty: just its 4-byte size)
    o.extend_from_slice(&4u32.to_le_bytes());
    o
}
