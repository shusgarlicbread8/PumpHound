use pumphound_domain::{Atomic, DecodeError, Pubkey};

pub struct BorshCursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BorshCursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if self.pos + n > self.data.len() {
            return Err(DecodeError::Borsh(format!(
                "need {n} bytes at {}, have {}",
                self.pos,
                self.data.len()
            )));
        }
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    pub fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }

    pub fn bool(&mut self) -> Result<bool, DecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(DecodeError::Borsh(format!(
                "invalid bool discriminant: {value}"
            ))),
        }
    }

    pub fn u16(&mut self) -> Result<u16, DecodeError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn u64(&mut self) -> Result<u64, DecodeError> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes(b.try_into().unwrap()))
    }

    pub fn i64(&mut self) -> Result<i64, DecodeError> {
        let b = self.take(8)?;
        Ok(i64::from_le_bytes(b.try_into().unwrap()))
    }

    pub fn i128(&mut self) -> Result<i128, DecodeError> {
        let b = self.take(16)?;
        Ok(i128::from_le_bytes(b.try_into().unwrap()))
    }

    pub fn atomic_u64(&mut self) -> Result<Atomic, DecodeError> {
        Ok(Atomic(self.u64()? as u128))
    }

    pub fn pubkey(&mut self) -> Result<Pubkey, DecodeError> {
        let b = self.take(32)?;
        let mut arr = [0u8; 32];
        arr.copy_from_slice(b);
        Ok(Pubkey(arr))
    }

    pub fn string(&mut self) -> Result<String, DecodeError> {
        let len = self.u32()? as usize;
        if len > 16 * 1024 {
            return Err(DecodeError::Borsh(format!("string too long: {len}")));
        }
        let b = self.take(len)?;
        String::from_utf8(b.to_vec()).map_err(|e| DecodeError::Borsh(e.to_string()))
    }

    pub fn u32(&mut self) -> Result<u32, DecodeError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes(b.try_into().unwrap()))
    }

    pub fn skip_vec_pubkey_u16(&mut self) -> Result<u32, DecodeError> {
        let n = self.u32()?;
        for _ in 0..n {
            let _ = self.pubkey()?;
            let _ = self.u16()?;
        }
        Ok(n)
    }

    pub fn skip_bytes(&mut self, n: usize) -> Result<(), DecodeError> {
        let _ = self.take(n)?;
        Ok(())
    }
}
