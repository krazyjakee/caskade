// Project-generated PE structures, never executed.
pub fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
pub fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
pub fn image(machine: u16, section: &str, resources: &[(u32, Vec<u8>)], overlay: &[u8]) -> Vec<u8> {
    let rsrc = resource_tree(resources);
    let raw_size = rsrc.len().max(512).div_ceil(512) * 512;
    let mut bytes = vec![0; 512 + raw_size];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    put16(&mut bytes, 132, machine);
    put16(&mut bytes, 134, 1);
    let optional_size = if machine == 0x14c { 224 } else { 240 };
    put16(&mut bytes, 148, optional_size);
    put16(&mut bytes, 150, 0x22);
    let opt = 152;
    let directories = if machine == 0x14c { 96 } else { 112 };
    put16(
        &mut bytes,
        opt,
        if machine == 0x14c { 0x10b } else { 0x20b },
    );
    put32(&mut bytes, opt + 32, 4096);
    put32(&mut bytes, opt + 36, 512);
    put32(&mut bytes, opt + 56, 8192);
    put32(&mut bytes, opt + 60, 512);
    put16(&mut bytes, opt + 68, 2);
    put32(&mut bytes, opt + directories - 4, 16);
    if !resources.is_empty() {
        put32(&mut bytes, opt + directories + 16, 4096);
        put32(&mut bytes, opt + directories + 20, rsrc.len() as u32);
    }
    let sec = opt + optional_size as usize;
    bytes[sec..sec + section.len()].copy_from_slice(section.as_bytes());
    put32(&mut bytes, sec + 8, raw_size as u32);
    put32(&mut bytes, sec + 12, 4096);
    put32(&mut bytes, sec + 16, raw_size as u32);
    put32(&mut bytes, sec + 20, 512);
    bytes[512..512 + rsrc.len()].copy_from_slice(&rsrc);
    bytes.extend_from_slice(overlay);
    bytes
}
fn resource_tree(resources: &[(u32, Vec<u8>)]) -> Vec<u8> {
    if resources.is_empty() {
        return vec![];
    }
    let mut bytes = vec![0; 16 + 8 * resources.len()];
    put16(&mut bytes, 14, resources.len() as u16);
    for (i, (kind, data)) in resources.iter().enumerate() {
        let second = bytes.len();
        bytes.resize(second + 24, 0);
        put16(&mut bytes, second + 14, 1);
        let third = bytes.len();
        bytes.resize(third + 24, 0);
        put16(&mut bytes, third + 14, 1);
        let entry = bytes.len();
        bytes.resize(entry + 16, 0);
        let payload = bytes.len();
        bytes.extend_from_slice(data);
        bytes.resize(bytes.len().div_ceil(4) * 4, 0);
        put32(&mut bytes, 16 + i * 8, *kind);
        put32(&mut bytes, 20 + i * 8, 0x80000000 | second as u32);
        put32(&mut bytes, second + 16, 1);
        put32(&mut bytes, second + 20, 0x80000000 | third as u32);
        put32(&mut bytes, third + 16, 1033);
        put32(&mut bytes, third + 20, entry as u32);
        put32(&mut bytes, entry, 4096 + payload as u32);
        put32(&mut bytes, entry + 4, data.len() as u32);
    }
    bytes
}
fn block(key: &str, value: &[u8], words: bool, children: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = vec![0; 6];
    put16(
        &mut bytes,
        2,
        if words { value.len() / 2 } else { value.len() } as u16,
    );
    put16(&mut bytes, 4, u16::from(words));
    for word in key.encode_utf16().chain([0]) {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.resize(bytes.len().div_ceil(4) * 4, 0);
    bytes.extend_from_slice(value);
    for child in children {
        bytes.resize(bytes.len().div_ceil(4) * 4, 0);
        bytes.extend_from_slice(child);
    }
    let len = bytes.len() as u16;
    put16(&mut bytes, 0, len);
    bytes
}
pub fn version(strings: &[(&str, &str)]) -> Vec<u8> {
    let children = strings
        .iter()
        .map(|(key, value)| {
            let value = value
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>();
            block(key, &value, true, &[])
        })
        .collect::<Vec<_>>();
    let table = block("040904b0", &[], true, &children);
    let strings = block("StringFileInfo", &[], true, &[table]);
    let translation = block("Translation", &[9, 4, 0xb0, 4], false, &[]);
    let vars = block("VarFileInfo", &[], true, &[translation]);
    let mut fixed = vec![0; 52];
    put32(&mut fixed, 0, 0xfeef04bd);
    put32(&mut fixed, 4, 0x10000);
    block("VS_VERSION_INFO", &fixed, false, &[strings, vars])
}
