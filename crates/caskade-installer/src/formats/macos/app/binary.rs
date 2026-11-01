use super::{AppError, Result};
use std::collections::BTreeSet;
fn invalid() -> AppError {
    AppError::Invalid("invalid binary plist object graph".into())
}
fn integer(data: &[u8], offset: usize, width: usize) -> Result<usize> {
    let bytes = data
        .get(offset..offset.checked_add(width).ok_or_else(invalid)?)
        .ok_or_else(invalid)?;
    let mut value = 0_usize;
    for byte in bytes {
        value = value
            .checked_mul(256)
            .and_then(|n| n.checked_add(usize::from(*byte)))
            .ok_or_else(invalid)?;
    }
    Ok(value)
}
/// Bound both depth and expansion of a binary plist DAG before Value's
/// recursive tree builder is called. Offsets and references stay in the input.
pub(super) fn validate(data: &[u8]) -> Result<()> {
    if data.len() < 40 {
        return Err(invalid());
    }
    let trailer = &data[data.len() - 32..];
    let width = usize::from(trailer[6]);
    let refs = usize::from(trailer[7]);
    let count = integer(trailer, 8, 8)?;
    let root = integer(trailer, 16, 8)?;
    let table = integer(trailer, 24, 8)?;
    if count == 0
        || count > 100_000
        || root >= count
        || !(1..=8).contains(&width)
        || !(1..=8).contains(&refs)
        || table < 8
        || count
            .checked_mul(width)
            .and_then(|n| table.checked_add(n))
            .is_none_or(|n| n > data.len() - 32)
    {
        return Err(invalid());
    }
    let mut stack = vec![(root, 0, false)];
    let mut ancestors = BTreeSet::new();
    let mut visited = 0;
    while let Some((id, depth, exit)) = stack.pop() {
        if exit {
            ancestors.remove(&id);
            continue;
        }
        visited += 1;
        if visited > 100_000 || depth > 64 {
            return Err(AppError::MetadataLimit);
        }
        if id >= count || !ancestors.insert(id) {
            return Err(invalid());
        }
        let offset = integer(data, table + id * width, width)?;
        if !(8..table).contains(&offset) {
            return Err(invalid());
        }
        let marker = data[offset];
        let ty = marker >> 4;
        let mut length = usize::from(marker & 15);
        let mut start = offset + 1;
        if matches!(ty, 4 | 5 | 6 | 10 | 13) && length == 15 {
            let marker = *data.get(start).ok_or_else(invalid)?;
            if marker >> 4 != 1 || marker & 15 > 3 {
                return Err(invalid());
            }
            let size = 1_usize << (marker & 15);
            length = integer(data, start + 1, size)?;
            start += 1 + size;
        }
        if matches!(ty, 4 | 5 | 6 | 10 | 13) {
            let units = match ty {
                6 => 2,
                10 => refs,
                13 => refs * 2,
                _ => 1,
            };
            if length
                .checked_mul(units)
                .and_then(|n| start.checked_add(n))
                .is_none_or(|n| n > table)
            {
                return Err(invalid());
            }
        }
        if matches!(ty, 10 | 13) {
            if length > 100_000 {
                return Err(AppError::MetadataLimit);
            }
            let children = length * if ty == 13 { 2 } else { 1 };
            stack.push((id, depth, true));
            for index in (0..children).rev() {
                stack.push((integer(data, start + index * refs, refs)?, depth + 1, false));
            }
        } else {
            ancestors.remove(&id);
        }
    }
    Ok(())
}
