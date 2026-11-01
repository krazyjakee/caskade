//! Allocation guards around upstream MSI/CFB readers. Actual MSI table and CAB
//! decoding remains upstream; this checks lengths that they allocate before I/O.
use std::{
    cell::Cell,
    io::{self, Read, Seek, SeekFrom},
    rc::Rc,
};

use super::{MsiError, MsiLimits, Result};

const POOL: &str = "\u{4840}\u{3f3f}\u{4577}\u{446c}\u{3e6a}\u{44b2}\u{482f}";
const DATA: &str = "\u{4840}\u{3f3f}\u{4577}\u{446c}\u{3b6a}\u{45e4}\u{4824}";
const SUMMARY: &str = "\u{5}SummaryInformation";

#[derive(Clone)]
pub(super) struct Budget(Rc<Cell<Option<u64>>>);
impl Budget {
    pub fn payload(&self) {
        self.0.set(None);
    }
}
pub(super) struct Bounded<R> {
    reader: R,
    budget: Budget,
}
impl<R: Read> Read for Bounded<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let length = self.budget.0.get().map_or(buffer.len(), |remaining| {
            buffer.len().min(remaining as usize)
        });
        if length == 0 && !buffer.is_empty() {
            return Err(io::Error::other("MSI metadata read budget exceeded"));
        }
        let count = self.reader.read(&mut buffer[..length])?;
        if let Some(remaining) = self.budget.0.get() {
            self.budget.0.set(Some(remaining - count as u64));
        }
        Ok(count)
    }
}
impl<R: Seek> Seek for Bounded<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.reader.seek(position)
    }
}

pub(super) fn open<R: Read + Seek>(
    reader: R,
    limits: MsiLimits,
) -> Result<(msi::Package<Bounded<R>>, Budget)> {
    let budget = Budget(Rc::new(Cell::new(Some(limits.max_read_bytes))));
    let mut bounded = Bounded {
        reader,
        budget: budget.clone(),
    };
    preflight(&mut bounded, limits)?;
    bounded.rewind()?;
    let package =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| msi::Package::open(bounded)))
            .map_err(|_| {
            MsiError::Invalid("upstream MSI parser rejected malformed metadata".into())
        })??;
    Ok((package, budget))
}

fn number(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset.checked_add(4).ok_or(MsiError::Limit)?)
            .ok_or_else(|| MsiError::Invalid("truncated MSI metadata".into()))?
            .try_into()
            .expect("four bytes"),
    ))
}

fn preflight(reader: &mut (impl Read + Seek), limits: MsiLimits) -> Result<()> {
    let size = reader.seek(SeekFrom::End(0))?;
    if size > limits.max_input_bytes {
        return Err(MsiError::Limit);
    }
    reader.rewind()?;
    let mut header = [0; 512];
    reader.read_exact(&mut header)?;
    if &header[..8] != b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1" {
        return Err(MsiError::Invalid("not a compound-file MSI".into()));
    }
    let shift = u16::from_le_bytes([header[30], header[31]]);
    if !matches!(shift, 9 | 12) {
        return Err(MsiError::Invalid(
            "invalid compound-file sector size".into(),
        ));
    }
    let sector = 1_u64 << shift;
    for offset in [40, 44, 64, 72] {
        if u64::from(number(&header, offset)?)
            .checked_mul(sector)
            .is_none_or(|bytes| bytes > limits.max_metadata_bytes)
        {
            return Err(MsiError::Limit);
        }
    }
    reader.rewind()?;
    let mut compound = cfb::CompoundFile::open(&mut *reader)?;
    let mut metadata_bytes = 0_u64;
    let mut entry_count = 0;
    for entry in compound.walk() {
        entry_count += 1;
        if entry_count > limits.max_rows {
            return Err(MsiError::Limit);
        }
        if entry.is_stream() && (entry.name().starts_with('\u{4840}') || entry.name() == SUMMARY) {
            metadata_bytes = metadata_bytes
                .checked_add(entry.len())
                .ok_or(MsiError::Limit)?;
            if metadata_bytes > limits.max_metadata_bytes {
                return Err(MsiError::Limit);
            }
        }
    }
    let data_length = compound.entry(DATA)?.len();
    let mut pool = vec![];
    compound
        .open_stream(POOL)?
        .take(limits.max_metadata_bytes.saturating_add(1))
        .read_to_end(&mut pool)?;
    if pool.len() as u64 > limits.max_metadata_bytes || pool.len() < 4 {
        return Err(MsiError::Limit);
    }
    let mut position = 4;
    let mut total = 0_u64;
    let mut count = 0;
    while position < pool.len() {
        let record = pool
            .get(position..position + 4)
            .ok_or_else(|| MsiError::Invalid("truncated string-pool record".into()))?;
        let mut length = u16::from_le_bytes([record[0], record[1]]) as u64;
        let references = u16::from_le_bytes([record[2], record[3]]);
        position += 4;
        if length == 0 && references > 0 {
            length = number(&pool, position)? as u64;
            position += 4;
        }
        total = total.checked_add(length).ok_or(MsiError::Limit)?;
        count += 1;
        if length > limits.max_string_bytes
            || total > data_length
            || total > limits.max_metadata_bytes
            || count > limits.max_rows
        {
            return Err(MsiError::Limit);
        }
    }
    let reference_width = if number(&pool, 0)? & 0x8000_0000 != 0 {
        3
    } else {
        2
    };
    // These three bootstrap tables have a fixed schema inside upstream's
    // Package::open. Reject null required cells before its unchecked unwraps;
    // actual table decoding and all application rows remain upstream.
    bootstrap(
        &mut compound,
        "\u{4840}\u{3f7f}\u{4164}\u{422f}\u{4836}",
        &[(reference_width, true, false)],
        count,
        limits,
    )?;
    bootstrap(
        &mut compound,
        "\u{4840}\u{3b3f}\u{43f2}\u{4438}\u{45b1}",
        &[
            (reference_width, true, false),
            (2, false, false),
            (reference_width, true, false),
            (2, false, false),
        ],
        count,
        limits,
    )?;
    bootstrap(
        &mut compound,
        "\u{4840}\u{3fff}\u{43e4}\u{41ec}\u{45e4}\u{44ac}\u{4831}",
        &[
            (reference_width, true, false),
            (reference_width, true, false),
            (reference_width, true, false),
            (4, false, true),
            (4, false, true),
            (reference_width, true, true),
            (2, false, true),
            (reference_width, true, true),
            (reference_width, true, true),
            (reference_width, true, true),
        ],
        count,
        limits,
    )?;
    let mut summary = vec![];
    compound
        .open_stream(SUMMARY)?
        .take(limits.max_metadata_bytes.saturating_add(1))
        .read_to_end(&mut summary)?;
    if summary.len() as u64 > limits.max_metadata_bytes {
        return Err(MsiError::Limit);
    }
    let section = number(&summary, 44)? as usize;
    let count = number(&summary, section.checked_add(4).ok_or(MsiError::Limit)?)? as usize;
    if count > limits.max_rows {
        return Err(MsiError::Limit);
    }
    for index in 0..count {
        let item = section
            .checked_add(8)
            .and_then(|start| start.checked_add(index.checked_mul(8)?))
            .ok_or(MsiError::Limit)?;
        let value = section
            .checked_add(number(&summary, item + 4)? as usize)
            .ok_or(MsiError::Limit)?;
        if number(&summary, value)? == 30 {
            let length = number(&summary, value + 4)? as usize;
            if length as u64 > limits.max_string_bytes
                || value
                    .checked_add(8)
                    .and_then(|start| start.checked_add(length))
                    .is_none_or(|end| end > summary.len())
            {
                return Err(MsiError::Limit);
            }
        }
    }
    Ok(())
}

fn bootstrap<F: Read + Seek>(
    compound: &mut cfb::CompoundFile<F>,
    name: &str,
    columns: &[(usize, bool, bool)],
    strings: usize,
    limits: MsiLimits,
) -> Result<()> {
    if !compound.is_stream(name) {
        return Ok(());
    }
    let mut bytes = vec![];
    compound
        .open_stream(name)?
        .take(limits.max_metadata_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    let row_size = columns.iter().map(|(width, _, _)| width).sum::<usize>();
    if bytes.len() as u64 > limits.max_metadata_bytes || !bytes.len().is_multiple_of(row_size) {
        return Err(MsiError::Invalid("invalid bootstrap table length".into()));
    }
    let rows = bytes.len() / row_size;
    if rows > 65_536 || rows > limits.max_rows {
        return Err(MsiError::Limit);
    }
    let mut offset = 0;
    for &(width, reference, nullable) in columns {
        for _ in 0..rows {
            let slice = bytes
                .get(offset..offset + width)
                .ok_or_else(|| MsiError::Invalid("truncated bootstrap cell".into()))?;
            let value = slice
                .iter()
                .enumerate()
                .fold(0_u32, |value, (index, byte)| {
                    value | (u32::from(*byte) << (index * 8))
                });
            if (!nullable && value == 0) || (reference && value as usize > strings) {
                return Err(MsiError::Invalid(
                    "null or invalid bootstrap table cell".into(),
                ));
            }
            offset += width;
        }
    }
    Ok(())
}
