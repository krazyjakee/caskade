use super::{Limits, ParseError, Result};

pub(crate) struct Cursor<'a> {
    pub data: &'a [u8],
    pub pos: usize,
    pub limits: &'a Limits,
}
impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8], limits: &'a Limits) -> Self {
        Self {
            data,
            pos: 0,
            limits,
        }
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or(ParseError::Truncated(self.pos))?;
        let out = self
            .data
            .get(self.pos..end)
            .ok_or(ParseError::Truncated(self.pos))?;
        self.pos = end;
        Ok(out)
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("eight bytes"),
        ))
    }
    pub fn cstr(&mut self) -> Result<String> {
        let rest = &self.data[self.pos..];
        let n = rest
            .iter()
            .take(self.limits.max_string_bytes.saturating_add(1))
            .position(|x| *x == 0)
            .ok_or({
                if rest.len() > self.limits.max_string_bytes {
                    ParseError::Limit {
                        budget: "string bytes",
                        maximum: self.limits.max_string_bytes,
                    }
                } else {
                    ParseError::Truncated(self.pos)
                }
            })?;
        let text = std::str::from_utf8(self.take(n)?)
            .map_err(|_| ParseError::Encoding)?
            .to_owned();
        self.take(1)?;
        Ok(text)
    }
    pub fn utf16(&mut self, n: usize) -> Result<String> {
        self.limits.check(
            n.saturating_mul(2),
            self.limits.max_string_bytes,
            "string bytes",
        )?;
        let words = self
            .take(n.checked_mul(2).ok_or(ParseError::Truncated(self.pos))?)?
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b))
            .collect::<Vec<_>>();
        String::from_utf16(&words).map_err(|_| ParseError::Encoding)
    }
}
