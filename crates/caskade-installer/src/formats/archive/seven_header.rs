//! A bounded 7z metadata preflight. The upstream reader has no configurable
//! allocation limits. Decode its header here, cap counts/dictionaries, then
//! expose a virtual uncompressed header to the upstream payload decoder.
use super::common::*;
use std::io::{Cursor, Read, Seek, SeekFrom};

fn invalid() -> ArchiveError {
    ArchiveError::Invalid("invalid or unsupported 7z metadata".into())
}
#[derive(Clone)]
struct Coder {
    id: Vec<u8>,
    props: Vec<u8>,
}
#[derive(Default)]
struct Block {
    coders: Vec<Coder>,
    order: Vec<usize>,
    sizes: Vec<u64>,
    crc: Option<u32>,
    streams: usize,
    packed_count: usize,
}
#[derive(Default)]
struct Streams {
    pos: u64,
    packed: Vec<u64>,
    blocks: Vec<Block>,
    count: usize,
}
struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    options: &'a ArchiveOptions,
}
impl<'a> Parser<'a> {
    fn new(bytes: &'a [u8], options: &'a ArchiveOptions) -> Self {
        Self {
            bytes,
            pos: 0,
            options,
        }
    }
    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(len).ok_or_else(invalid)?;
        let value = self.bytes.get(self.pos..end).ok_or_else(invalid)?;
        self.pos = end;
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn expect(&mut self, expected: u8) -> Result<()> {
        if self.byte()? != expected {
            Err(invalid())
        } else {
            Ok(())
        }
    }
    fn number(&mut self) -> Result<u64> {
        let first = self.byte()?;
        let mut value = 0;
        for i in 0..8 {
            let mask = 0x80 >> i;
            if first & mask == 0 {
                return Ok(value | (u64::from(first & (mask - 1)) << (8 * i)));
            }
            value |= u64::from(self.byte()?) << (8 * i);
        }
        Ok(value)
    }
    fn count(&mut self, limit: usize) -> Result<usize> {
        let value = self.number()?;
        if value > limit as u64 {
            return Err(ArchiveError::MemoryLimit);
        }
        Ok(value as usize)
    }
    fn flags(&mut self, count: usize, all: bool) -> Result<Vec<bool>> {
        if all && self.byte()? != 0 {
            return Ok(vec![true; count]);
        }
        let data = self.take(count.div_ceil(8))?;
        Ok((0..count)
            .map(|i| data[i / 8] & (0x80 >> (i % 8)) != 0)
            .collect())
    }
    fn digests(&mut self, count: usize) -> Result<Vec<Option<u32>>> {
        let flags = self.flags(count, true)?;
        flags
            .into_iter()
            .map(|present| {
                if present {
                    Ok(Some(u32::from_le_bytes(self.take(4)?.try_into().unwrap())))
                } else {
                    Ok(None)
                }
            })
            .collect()
    }
    fn block(&mut self) -> Result<Block> {
        let count = self.count(16)?;
        if count == 0 {
            return Err(invalid());
        }
        let mut block = Block::default();
        let mut inputs = Vec::new();
        let mut starts = Vec::new();
        let mut total_inputs = 0;
        let mut dictionaries = 0_u64;
        for _ in 0..count {
            let flags = self.byte()?;
            if flags & 0x80 != 0 || flags & 15 == 0 {
                return Err(invalid());
            }
            let id = self.take((flags & 15) as usize)?.to_vec();
            let (num_in, num_out) = if flags & 0x10 != 0 {
                (self.count(4)?, self.count(1)?)
            } else {
                (1, 1)
            };
            if num_out != 1 || !matches!(num_in, 1 | 4) || (num_in == 4 && id != [3, 3, 1, 0x1b]) {
                return Err(ArchiveError::Unsupported("7z multi-stream coder".into()));
            }
            starts.push(total_inputs);
            inputs.push(num_in);
            total_inputs += num_in;
            let props = if flags & 0x20 != 0 {
                let len = self.count(64)?;
                self.take(len)?.to_vec()
            } else {
                vec![]
            };
            check_coder(&id, &props, self.options)?;
            let memory = match id.as_slice() {
                [0x21] | [3, 1, 1] => u64::from(dictionary(&id, &props)?),
                [3, 4, 1] => u32::from_le_bytes(props[1..5].try_into().unwrap()) as u64,
                _ => 0,
            };
            dictionaries = dictionaries
                .checked_add(memory)
                .ok_or(ArchiveError::MemoryLimit)?;
            if dictionaries > self.options.max_decoder_bytes {
                return Err(ArchiveError::MemoryLimit);
            }
            block.coders.push(Coder { id, props });
        }
        let mut binding = vec![None; total_inputs];
        let mut used_output = vec![false; count];
        for _ in 0..count - 1 {
            let input = self.count(total_inputs - 1)?;
            let output = self.count(count - 1)?;
            if binding[input].is_some() || used_output[output] {
                return Err(invalid());
            }
            binding[input] = Some(output);
            used_output[output] = true;
        }
        let packed: Vec<_> = binding
            .iter()
            .enumerate()
            .filter_map(|(i, b)| b.is_none().then_some(i))
            .collect();
        block.packed_count = packed.len();
        if packed.len() > 1 {
            let mut seen = std::collections::BTreeSet::new();
            for _ in &packed {
                let index = self.count(total_inputs - 1)?;
                if binding[index].is_some() || !seen.insert(index) {
                    return Err(invalid());
                }
            }
        }
        let main = used_output.iter().position(|b| !*b).ok_or_else(invalid)?;
        fn visit(
            index: usize,
            starts: &[usize],
            inputs: &[usize],
            binding: &[Option<usize>],
            state: &mut [u8],
        ) -> Result<()> {
            if state[index] == 1 {
                return Err(invalid());
            }
            if state[index] == 2 {
                return Ok(());
            }
            state[index] = 1;
            for input in starts[index]..starts[index] + inputs[index] {
                if let Some(source) = binding[input] {
                    visit(source, starts, inputs, binding, state)?;
                }
            }
            state[index] = 2;
            Ok(())
        }
        let mut state = vec![0; count];
        visit(main, &starts, &inputs, &binding, &mut state)?;
        if state.contains(&0) {
            return Err(invalid());
        }
        block.order.push(main);
        Ok(block)
    }
    fn streams(&mut self) -> Result<Streams> {
        let mut streams = Streams::default();
        let mut tag = self.byte()?;
        if tag == 6 {
            streams.pos = self.number()?;
            let count = self.count(self.options.max_entries)?;
            self.expect(9)?;
            for _ in 0..count {
                streams.packed.push(self.number()?);
            }
            tag = self.byte()?;
            if tag == 10 {
                self.digests(count)?;
                tag = self.byte()?;
            }
            if tag != 0 {
                return Err(invalid());
            }
            tag = self.byte()?;
        }
        if tag == 7 {
            self.expect(11)?;
            let count = self.count(self.options.max_entries)?;
            self.expect(0)?;
            for _ in 0..count {
                streams.blocks.push(self.block()?);
            }
            self.expect(12)?;
            for block in &mut streams.blocks {
                for _ in &block.coders {
                    let size = self.number()?;
                    if size > self.options.max_unpacked_bytes {
                        return Err(crate::safety::SafetyError::ByteLimit.into());
                    }
                    block.sizes.push(size);
                }
                block.streams = 1;
            }
            tag = self.byte()?;
            if tag == 10 {
                let crcs = self.digests(count)?;
                for (block, crc) in streams.blocks.iter_mut().zip(crcs) {
                    block.crc = crc;
                }
                tag = self.byte()?;
            }
            if tag != 0 {
                return Err(invalid());
            }
            tag = self.byte()?;
        }
        if tag == 8 {
            tag = self.byte()?;
            if tag == 13 {
                for block in &mut streams.blocks {
                    block.streams = self.count(self.options.max_entries)?;
                }
                tag = self.byte()?;
            }
            let count = streams
                .blocks
                .iter()
                .try_fold(0_usize, |a, b| a.checked_add(b.streams))
                .ok_or(ArchiveError::MemoryLimit)?;
            if count > self.options.max_entries {
                return Err(ArchiveError::MemoryLimit);
            }
            if tag == 9 {
                for block in &streams.blocks {
                    let mut sum = 0_u64;
                    for _ in 1..block.streams {
                        sum = sum.checked_add(self.number()?).ok_or_else(invalid)?;
                    }
                    let size = block.sizes[*block.order.last().ok_or_else(invalid)?];
                    if sum > size {
                        return Err(invalid());
                    }
                }
                tag = self.byte()?;
            } else if streams.blocks.iter().any(|b| b.streams > 1) {
                return Err(invalid());
            }
            if tag == 10 {
                self.digests(
                    streams
                        .blocks
                        .iter()
                        .filter(|b| b.streams != 1 || b.crc.is_none())
                        .map(|b| b.streams)
                        .sum(),
                )?;
                tag = self.byte()?;
            }
            if tag != 0 {
                return Err(invalid());
            }
            streams.count = count;
            tag = self.byte()?;
        } else {
            streams.count = streams.blocks.len();
        }
        if tag != 0
            || streams.packed.len() != streams.blocks.iter().map(|b| b.packed_count).sum::<usize>()
        {
            return Err(invalid());
        }
        Ok(streams)
    }
    fn header(&mut self, source_length: u64) -> Result<()> {
        self.expect(1)?;
        let mut tag = self.byte()?;
        if tag == 2 {
            loop {
                if self.byte()? == 0 {
                    break;
                }
                let size = self.count(self.options.max_metadata_bytes)?;
                self.take(size)?;
            }
            tag = self.byte()?;
        }
        let streams = if tag == 4 {
            let streams = self.streams()?;
            tag = self.byte()?;
            streams
        } else {
            Streams::default()
        };
        let packed_end = streams.packed.iter().try_fold(
            streams.pos.checked_add(32).ok_or_else(invalid)?,
            |sum, size| sum.checked_add(*size).ok_or_else(invalid),
        )?;
        if packed_end > source_length {
            return Err(invalid());
        }
        let unpacked = streams.blocks.iter().try_fold(0_u64, |sum, block| {
            sum.checked_add(block.sizes[*block.order.last().ok_or_else(invalid)?])
                .ok_or_else(invalid)
        })?;
        if unpacked > self.options.max_unpacked_bytes {
            return Err(crate::safety::SafetyError::ByteLimit.into());
        }
        if tag == 5 {
            let count = self.count(self.options.max_entries)?;
            let mut empty = 0;
            let mut names_seen = false;
            loop {
                let property = self.byte()?;
                if property == 0 {
                    break;
                }
                let length = self.count(self.options.max_metadata_bytes)?;
                let value = self.take(length)?;
                let mut field = Parser::new(value, self.options);
                match property {
                    14 => {
                        empty = field
                            .flags(count, false)?
                            .into_iter()
                            .filter(|b| *b)
                            .count();
                    }
                    15 | 16 => {
                        field.flags(empty, false)?;
                    }
                    17 => {
                        field.expect(0)?;
                        let raw = field.take(value.len().saturating_sub(1))?;
                        if raw.len() % 2 != 0 {
                            return Err(invalid());
                        }
                        let names = raw
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .filter(|c| **c == [0, 0])
                            .count();
                        if names != count || (count > 0 && !raw.ends_with(&[0, 0])) {
                            return Err(invalid());
                        }
                        names_seen = true;
                    }
                    18..=21 => {
                        let present = field.flags(count, true)?.into_iter().filter(|b| *b).count();
                        field.expect(0)?;
                        field.take(present * if property == 21 { 4 } else { 8 })?;
                    }
                    24 => {
                        return Err(ArchiveError::Unsupported(
                            "7z start position metadata".into(),
                        ));
                    }
                    _ => {
                        field.take(value.len())?;
                    }
                }
                if field.pos != value.len() {
                    return Err(invalid());
                }
            }
            if (count > 0 && !names_seen) || count - empty != streams.count {
                return Err(invalid());
            }
            tag = self.byte()?;
        } else if streams.count > 0 {
            return Err(invalid());
        }
        if tag != 0 || self.pos != self.bytes.len() {
            return Err(invalid());
        }
        Ok(())
    }
}
fn dictionary(id: &[u8], props: &[u8]) -> Result<u32> {
    match id {
        [0x21] if props.len() == 1 && props[0] <= 40 => Ok(if props[0] == 40 {
            u32::MAX
        } else {
            (2 | (u32::from(props[0]) & 1)) << (u32::from(props[0]) / 2 + 11)
        }),
        [3, 1, 1] if props.len() == 5 => Ok(u32::from_le_bytes(props[1..5].try_into().unwrap())),
        _ => Err(invalid()),
    }
}
fn check_coder(id: &[u8], props: &[u8], options: &ArchiveOptions) -> Result<()> {
    match id {
        [0x21] | [3, 1, 1] => {
            if u64::from(dictionary(id, props)?) > options.max_decoder_bytes {
                return Err(ArchiveError::MemoryLimit);
            }
            if id == [3, 1, 1] && props[0] >= 9 * 5 * 5 {
                return Err(invalid());
            }
        }
        [3, 4, 1] => {
            if props.len() != 5
                || u32::from_le_bytes(props[1..].try_into().unwrap()) as u64
                    > options.max_decoder_bytes
            {
                return Err(ArchiveError::MemoryLimit);
            }
        }
        [6, 0xf1, 7, 1] => {
            return Err(ArchiveError::Unsupported(
                "encrypted 7z requires a password".into(),
            ));
        }
        [0]
        | [4, 1, 8]
        | [4, 2, 2]
        | [3, 3, 1, 0x1b]
        | [3, 3, 1, 3]
        | [3, 3, 2, 5]
        | [3, 3, 4, 1]
        | [3, 3, 5, 1]
        | [3, 3, 7, 1]
        | [3, 3, 8, 5]
        | [3] => (),
        _ => return Err(ArchiveError::Unsupported(format!("7z coder {id:x?}"))),
    }
    Ok(())
}
fn crc(bytes: &[u8]) -> u32 {
    let mut value = !0_u32;
    for byte in bytes {
        value ^= u32::from(*byte);
        for _ in 0..8 {
            value = (value >> 1) ^ (0xedb88320 & (0_u32.wrapping_sub(value & 1)));
        }
    }
    !value
}

pub(super) struct Prepared<R> {
    reader: R,
    prefix: [u8; 32],
    header: Vec<u8>,
    original: u64,
    position: u64,
}
impl<R: Read + Seek> Prepared<R> {
    pub fn new(mut reader: R, options: &ArchiveOptions) -> Result<Self> {
        let original = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(0))?;
        let mut prefix = [0_u8; 32];
        reader.read_exact(&mut prefix)?;
        if &prefix[..6] != b"7z\xbc\xaf\x27\x1c" || prefix[6] != 0 {
            return Err(invalid());
        }
        if crc(&prefix[12..]) != u32::from_le_bytes(prefix[8..12].try_into().unwrap()) {
            return Err(invalid());
        }
        let offset = u64::from_le_bytes(prefix[12..20].try_into().unwrap());
        let length = u64::from_le_bytes(prefix[20..28].try_into().unwrap());
        if length > options.max_metadata_bytes as u64 {
            return Err(ArchiveError::MemoryLimit);
        }
        let offset = offset.checked_add(32).ok_or_else(invalid)?;
        if offset.checked_add(length).is_none_or(|end| end > original) {
            return Err(invalid());
        }
        reader.seek(SeekFrom::Start(offset))?;
        let mut header = vec![0; length as usize];
        reader.read_exact(&mut header)?;
        if crc(&header) != u32::from_le_bytes(prefix[28..32].try_into().unwrap()) {
            return Err(invalid());
        }
        if header.first() == Some(&23) {
            let mut parser = Parser::new(&header[1..], options);
            let streams = parser.streams()?;
            if parser.pos != header.len() - 1
                || streams.blocks.len() != 1
                || streams.packed.len() != 1
            {
                return Err(invalid());
            }
            let block = &streams.blocks[0];
            if block.coders.len() != 1 {
                return Err(ArchiveError::Unsupported(
                    "filtered or encrypted 7z header".into(),
                ));
            }
            let size = block.sizes[0];
            if size > options.max_metadata_bytes as u64
                || streams.packed[0] > options.max_metadata_bytes as u64
            {
                return Err(ArchiveError::MemoryLimit);
            }
            let start = streams.pos.checked_add(32).ok_or_else(invalid)?;
            if start
                .checked_add(streams.packed[0])
                .is_none_or(|end| end > original)
            {
                return Err(invalid());
            }
            reader.seek(SeekFrom::Start(start))?;
            let mut packed = vec![0; streams.packed[0] as usize];
            reader.read_exact(&mut packed)?;
            let coder = &block.coders[0];
            let input = Cursor::new(packed);
            let decoder: Box<dyn Read> = match coder.id.as_slice() {
                [0] => Box::new(input),
                [0x21] => Box::new(lzma_rust2::Lzma2Reader::new(
                    input,
                    dictionary(&coder.id, &coder.props)?,
                    None,
                )),
                [3, 1, 1] => Box::new(lzma_rust2::LzmaReader::new_with_props(
                    input,
                    size,
                    coder.props[0],
                    dictionary(&coder.id, &coder.props)?,
                    None,
                )?),
                _ => {
                    return Err(ArchiveError::Unsupported(
                        "7z header compression method".into(),
                    ));
                }
            };
            let mut decoded = Vec::new();
            decoder.take(size + 1).read_to_end(&mut decoded)?;
            if decoded.len() as u64 != size {
                return Err(invalid());
            }
            if let Some(expected) = block.crc
                && crc(&decoded) != expected
            {
                return Err(invalid());
            }
            header = decoded;
        }
        Parser::new(&header, options).header(original)?;
        prefix[12..20]
            .copy_from_slice(&original.checked_sub(32).ok_or_else(invalid)?.to_le_bytes());
        prefix[20..28].copy_from_slice(&(header.len() as u64).to_le_bytes());
        prefix[28..32].copy_from_slice(&crc(&header).to_le_bytes());
        let start_crc = crc(&prefix[12..]);
        prefix[8..12].copy_from_slice(&start_crc.to_le_bytes());
        Ok(Self {
            reader,
            prefix,
            header,
            original,
            position: 0,
        })
    }
}
impl<R: Read + Seek> Read for Prepared<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let count = if self.position < 32 {
            let count = buffer.len().min(32 - self.position as usize);
            buffer[..count].copy_from_slice(
                &self.prefix[self.position as usize..self.position as usize + count],
            );
            count
        } else if self.position < self.original {
            self.reader.seek(SeekFrom::Start(self.position))?;
            let count = buffer.len().min((self.original - self.position) as usize);
            self.reader.read(&mut buffer[..count])?
        } else {
            let index = self.position - self.original;
            if index >= self.header.len() as u64 {
                return Ok(0);
            }
            let count = buffer.len().min(self.header.len() - index as usize);
            buffer[..count].copy_from_slice(&self.header[index as usize..index as usize + count]);
            count
        };
        self.position += count as u64;
        Ok(count)
    }
}
impl<R: Read + Seek> Seek for Prepared<R> {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        let value = match from {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::Current(n) => i128::from(self.position) + i128::from(n),
            SeekFrom::End(n) => {
                i128::from(self.original) + self.header.len() as i128 + i128::from(n)
            }
        };
        self.position = u64::try_from(value)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid seek"))?;
        Ok(self.position)
    }
}
