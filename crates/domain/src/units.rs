use crate::DomainError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

/// Unsigned atomic token/quote quantity. JSON and wire format are decimal strings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Atomic(pub u128);

/// Signed ledger movement. JSON is a signed decimal string.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AtomicSigned(pub i128);

impl Atomic {
    pub const ZERO: Self = Self(0);

    pub fn new(v: u128) -> Self {
        Self(v)
    }

    pub fn checked_add(self, other: Self) -> Result<Self, DomainError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(DomainError::Overflow("atomic add"))
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, DomainError> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or(DomainError::Overflow("atomic sub"))
    }

    pub fn checked_mul_u64(self, rhs: u64) -> Result<Self, DomainError> {
        self.0
            .checked_mul(rhs as u128)
            .map(Self)
            .ok_or(DomainError::Overflow("atomic mul"))
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl AtomicSigned {
    pub fn from_unsigned(v: Atomic) -> Result<Self, DomainError> {
        i128::try_from(v.0)
            .map(Self)
            .map_err(|_| DomainError::Overflow("atomic to signed"))
    }

    pub fn negate(self) -> Result<Self, DomainError> {
        self.0
            .checked_neg()
            .map(Self)
            .ok_or(DomainError::Overflow("signed negate"))
    }

    pub fn checked_add(self, other: Self) -> Result<Self, DomainError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(DomainError::Overflow("signed add"))
    }
}

impl fmt::Display for Atomic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for AtomicSigned {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Atomic {
    type Err = DomainError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() || s.starts_with('-') || s.contains('.') {
            return Err(DomainError::InvalidAtomic(s.to_string()));
        }
        s.parse::<u128>()
            .map(Self)
            .map_err(|_| DomainError::InvalidAtomic(s.to_string()))
    }
}

impl FromStr for AtomicSigned {
    type Err = DomainError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<i128>()
            .map(Self)
            .map_err(|_| DomainError::InvalidAtomic(s.to_string()))
    }
}

impl Serialize for Atomic {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Atomic {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl Serialize for AtomicSigned {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for AtomicSigned {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Pubkey(pub [u8; 32]);

impl Pubkey {
    pub fn from_base58(s: &str) -> Result<Self, DomainError> {
        let bytes = bs58::decode(s)
            .into_vec()
            .map_err(|_| DomainError::InvalidPubkey(s.to_string()))?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| DomainError::InvalidPubkey(s.to_string()))?;
        Ok(Self(arr))
    }

    pub fn to_base58(self) -> String {
        bs58::encode(self.0).into_string()
    }
}

impl fmt::Display for Pubkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_base58())
    }
}

impl Serialize for Pubkey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_base58())
    }
}

impl<'de> Deserialize<'de> for Pubkey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Pubkey::from_base58(&s).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Signature(pub [u8; 64]);

impl Signature {
    pub fn from_base58(s: &str) -> Result<Self, DomainError> {
        let bytes = bs58::decode(s)
            .into_vec()
            .map_err(|_| DomainError::InvalidSignature(s.to_string()))?;
        let arr: [u8; 64] = bytes
            .try_into()
            .map_err(|_| DomainError::InvalidSignature(s.to_string()))?;
        Ok(Self(arr))
    }

    pub fn to_base58(self) -> String {
        bs58::encode(self.0).into_string()
    }
}

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_base58())
    }
}

impl Serialize for Signature {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_base58())
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Signature::from_base58(&s).map_err(serde::de::Error::custom)
    }
}

/// Slot as a typed u64. JSON is a decimal string.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Slot(pub u64);

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Slot {
    type Err = DomainError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<u64>()
            .map(Self)
            .map_err(|_| DomainError::InvalidAtomic(format!("slot {s}")))
    }
}

impl Serialize for Slot {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Slot {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetAmount {
    pub mint: Pubkey,
    pub atomic: Atomic,
    pub decimals: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_rejects_javascript_number_shape() {
        assert!(Atomic::from_str("-1").is_err());
        assert!(Atomic::from_str("1.2").is_err());
        assert_eq!(Atomic::from_str("10000000").unwrap().0, 10_000_000);
        let json = serde_json::to_string(&Atomic(42)).unwrap();
        assert_eq!(json, "\"42\"");
    }
}
