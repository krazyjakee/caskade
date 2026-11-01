/// A minimal PE32 containing only .rsrc, whose RT_ICON and RT_GROUP_ICON
/// entries hold `png` as a 16 px icon.
/// The generated file contains no runnable code and never needs a PE compiler.
pub fn pe_with_icon(png: &[u8]) -> Vec<u8> {
    fn u16_at(data: &mut [u8], at: usize, value: u16) {
        data[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn u32_at(data: &mut [u8], at: usize, value: u32) {
        data[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let group_at = (160 + png.len()).next_multiple_of(4);
    let resource_len = group_at + 20;
    let mut rsrc = vec![0; resource_len];
    u16_at(&mut rsrc, 14, 2);
    // Type -> resource ID -> language -> data entry.
    for (entry, id, offset) in [(16, 3, 32), (24, 14, 56), (48, 1, 80), (72, 1, 104)] {
        u32_at(&mut rsrc, entry, id);
        u32_at(&mut rsrc, entry + 4, 0x8000_0000 | offset);
    }
    for offset in [32, 56, 80, 104] {
        u16_at(&mut rsrc, offset + 14, 1);
    }
    for (entry, data) in [(96, 128), (120, 144)] {
        u32_at(&mut rsrc, entry, 1033);
        u32_at(&mut rsrc, entry + 4, data);
    }
    u32_at(&mut rsrc, 128, 0x1000 + 160);
    u32_at(&mut rsrc, 132, png.len() as u32);
    u32_at(&mut rsrc, 144, 0x1000 + group_at as u32);
    u32_at(&mut rsrc, 148, 20);
    rsrc[160..160 + png.len()].copy_from_slice(png);
    let group = &mut rsrc[group_at..];
    u16_at(group, 2, 1);
    u16_at(group, 4, 1);
    group[6] = 16;
    group[7] = 16;
    u16_at(group, 10, 1);
    u16_at(group, 12, 32);
    u32_at(group, 14, png.len() as u32);
    u16_at(group, 18, 1);

    let mut pe = vec![0; 512 + resource_len.next_multiple_of(512)];
    pe[..2].copy_from_slice(b"MZ");
    u32_at(&mut pe, 60, 128);
    pe[128..132].copy_from_slice(b"PE\0\0");
    u16_at(&mut pe, 132, 0x14c);
    u16_at(&mut pe, 134, 1);
    u16_at(&mut pe, 148, 224);
    u16_at(&mut pe, 150, 0x102);
    let optional = 152;
    u16_at(&mut pe, optional, 0x10b);
    u32_at(&mut pe, optional + 28, 0x400000);
    u32_at(&mut pe, optional + 32, 0x1000);
    u32_at(&mut pe, optional + 36, 512);
    u32_at(&mut pe, optional + 56, 0x2000);
    u32_at(&mut pe, optional + 60, 512);
    u16_at(&mut pe, optional + 68, 2);
    u32_at(&mut pe, optional + 92, 16);
    u32_at(&mut pe, optional + 112, 0x1000);
    u32_at(&mut pe, optional + 116, resource_len as u32);
    let section = 376;
    pe[section..section + 5].copy_from_slice(b".rsrc");
    u32_at(&mut pe, section + 8, resource_len as u32);
    u32_at(&mut pe, section + 12, 0x1000);
    u32_at(
        &mut pe,
        section + 16,
        resource_len.next_multiple_of(512) as u32,
    );
    u32_at(&mut pe, section + 20, 512);
    u32_at(&mut pe, section + 36, 0x40000040);
    pe[512..512 + resource_len].copy_from_slice(&rsrc);
    pe
}
