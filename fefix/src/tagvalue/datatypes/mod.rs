//! Types for holding [`DataType`](crate::DataType) values.
//!
//! This module introduces reusable, allocation-free data structures that can be
//! used to store [`DataType`](crate::DataType) values. This is done via the
//! [`DataType`] trait, which allows both serialization and deserialization.
//!
//! FerrumFIX maps FIX date types to the following [`DataType`] implementors:
//!
//! - `int`: any Rust primitive integer type;
//! - `Length`: [`usize`];
//! - `NumInGroup`: [`u16`];
//! - `SeqNum`: [`u64`];
//! - `TagNum`: [`u32`];
//! - `DayOfMonth`: [`u32`];
//! - `float`, `Qty`, `Price`, `PriceOffset`, `Amt`, `Percentage`: [`rust_decimal::Decimal`];
//! - `char`: [`u8`] (FIX mandates the use of a single-byte encoding, so `u8` is
//! a better fit than `char`);
//! - `Boolean`: [`bool`];
//! - `String`, `data`: `&[u8]`;
//! - `MultipleCharValue`: [`MultipleChars`];
//! - `MultipleValueString`: [`MultipleStrings`];
//! - `Country`: `[u8; 2]`;
//! - `Currency`: `[u8; 3]`;
//! - `Exchange`: `[u8; 4]`;
//! - `month-year`: [`MonthYear`];
//! - `UTCTimestamp`, `LocalMktDate`: [`Timestamp`];
//! - `UTCTimeOnly`: [`Time`];
//! - `UTCDateOnly`: [`Date`];
//! - `TZTimeOnly`: [`TzTime`];
//! - `TZTimestamp`: [`TzTimestamp`];
//!
//! # Quick tour of [`DataType`]
//!
//! ```
//! use fefix::datatypes::{DataType, Timestamp};
//!
//! let bytes = b"20130422-12:30:00.000";
//!
//! // You can use `DataType::deserialize` to parse data fields.
//! let timestamp = Timestamp::deserialize(bytes).unwrap();
//! assert_eq!(timestamp.date().year(), 2013);
//!
//! // `DataType::deserialize_lossy` is like `DataType::deserialize`, but it's
//! // allowed to skip some format verification for the sake of speed.
//! assert!(u32::deserialize(b"invalid integer").is_err());
//! assert!(u32::deserialize_lossy(b"invalid integer").is_ok());
//!
//! let buffer: &mut Vec<u8> = &mut vec![];
//! // Use `DataType::serialize` to write values to buffers.
//! 1337u32.serialize(buffer);
//! assert_eq!(&buffer[..], b"1337" as &[u8]);
//! ```

mod checksum;
mod date;
pub mod error;
mod monthyear;
mod multiple_chars;
mod multiple_strings;
mod time;
mod timestamp;
mod tz;
mod tz_time;
mod tz_timestamp;

pub use checksum::CheckSum;
pub use date::Date;
pub use monthyear::MonthYear;
pub use multiple_chars::MultipleChars;
pub use multiple_strings::MultipleStrings;
pub use time::Time;
pub use timestamp::Timestamp;
pub use tz::Tz;
pub use tz_time::TzTime;
pub use tz_timestamp::TzTimestamp;

use crate::{Buffer, TagU16};
use rust_decimal::Decimal;
use std::convert::TryInto;
use std::str::FromStr;

/// A trait for serializing data directly into a [`Buffer`].
pub trait FixFieldValue<'a>
where
    Self: Sized,
{
    type Error;
    type SerializeSettings;

    fn serialize<B>(&self, buffer: &mut B) -> usize
    where
        B: Buffer;

    fn serialize_with<B>(&self, buffer: &mut B, _settings: &Self::SerializeSettings) -> usize
    where
        B: Buffer,
    {
        self.serialize(buffer)
    }

    fn deserialize(data: &'a [u8]) -> Result<Self, Self::Error>;

    fn deserialize_lossy(data: &'a [u8]) -> Result<Self, Self::Error> {
        Self::deserialize(data)
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Padding {
    pub len: usize,
    pub byte: u8,
}

impl Padding {
    pub fn zeros(len: usize) -> Self {
        Self { len, byte: b'0' }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct WithMilliseconds(pub bool);

impl<'a> FixFieldValue<'a> for chrono::DateTime<chrono::Utc> {
    type Error = ();
    type SerializeSettings = WithMilliseconds;

    #[inline(always)]
    fn serialize<B>(&self, buffer: &mut B) -> usize
    where
        B: Buffer,
    {
        // Serialize with milliseconds by default.
        self.serialize_with(buffer, &WithMilliseconds(true))
    }

    #[inline(always)]
    fn serialize_with<B>(&self, buffer: &mut B, settings: &Self::SerializeSettings) -> usize
    where
        B: Buffer,
    {
        use chrono::{Datelike, Timelike};
        (self.year() as u32).serialize_with(buffer, &Padding::zeros(4));
        (self.month() as u32).serialize_with(buffer, &Padding::zeros(2));
        (self.day() as u32).serialize_with(buffer, &Padding::zeros(2));
        buffer.extend_from_slice(b"-");
        (self.hour() as u32).serialize_with(buffer, &Padding::zeros(2));
        buffer.extend_from_slice(b":");
        (self.minute() as u32).serialize_with(buffer, &Padding::zeros(2));
        buffer.extend_from_slice(b":");
        (self.second() as u32).serialize_with(buffer, &Padding::zeros(2));
        if settings.0 {
            buffer.extend_from_slice(b".");
            (self.nanosecond() / 10E6 as u32).serialize_with(buffer, &Padding::zeros(3));
            21
        } else {
            17
        }
    }

    #[inline(always)]
    fn deserialize(_data: &'a [u8]) -> Result<Self, Self::Error> {
        Err(())
    }
}

impl<'a> FixFieldValue<'a> for Decimal {
    type Error = error::Decimal;
    type SerializeSettings = ();

    #[inline(always)]
    fn serialize<B>(&self, buffer: &mut B) -> usize
    where
        B: Buffer,
    {
        // TODO: Remove allocations.
        let s = self.to_string();
        buffer.extend_from_slice(s.as_bytes());
        s.as_bytes().len()
    }

    #[inline(always)]
    fn deserialize(data: &'a [u8]) -> Result<Self, Self::Error> {
        let s = std::str::from_utf8(data).map_err(|_| Self::Error::NotUtf8)?;
        Decimal::from_str(s).map_err(|err| Self::Error::Other(err.to_string()))
    }
}

impl<'a> FixFieldValue<'a> for bool {
    type Error = error::Bool;
    type SerializeSettings = ();

    #[inline(always)]
    fn serialize<B>(&self, buffer: &mut B) -> usize
    where
        B: Buffer,
    {
        let byte = if *self { b'Y' } else { b'N' };
        buffer.extend_from_slice(&[byte]);
        1
    }

    #[inline(always)]
    fn deserialize(data: &'a [u8]) -> Result<Self, Self::Error> {
        if data.len() != 1 {
            Err(Self::Error::WrongLength)
        } else if data[0] == b'Y' {
            Ok(true)
        } else if data[0] == b'N' {
            Ok(false)
        } else {
            Err(Self::Error::InvalidCharacter)
        }
    }

    #[inline(always)]
    fn deserialize_lossy(data: &'a [u8]) -> Result<Self, Self::Error> {
        if data.len() != 1 {
            Err(Self::Error::WrongLength)
        } else {
            Ok(data[0] == b'Y')
        }
    }
}

impl<'a> FixFieldValue<'a> for &'a str {
    type Error = std::str::Utf8Error;
    type SerializeSettings = ();

    #[inline(always)]
    fn serialize<B>(&self, buffer: &mut B) -> usize
    where
        B: Buffer,
    {
        buffer.extend_from_slice(self.as_bytes());
        self.as_bytes().len()
    }

    #[inline(always)]
    fn deserialize(data: &'a [u8]) -> Result<Self, Self::Error> {
        std::str::from_utf8(data)
    }
}

impl<'a> FixFieldValue<'a> for u8 {
    type Error = error::Int;
    type SerializeSettings = ();

    #[inline(always)]
    fn serialize<B>(&self, buffer: &mut B) -> usize
    where
        B: Buffer,
    {
        buffer.extend_from_slice(&[*self]);
        1
    }

    #[inline(always)]
    fn deserialize(data: &'a [u8]) -> Result<Self, Self::Error> {
        Ok(data[0])
    }
}

impl<'a> FixFieldValue<'a> for &'a [u8] {
    type Error = ();
    type SerializeSettings = ();

    #[inline(always)]
    fn serialize<B>(&self, buffer: &mut B) -> usize
    where
        B: Buffer,
    {
        buffer.extend_from_slice(self);
        self.len()
    }

    #[inline(always)]
    fn deserialize(data: &'a [u8]) -> Result<Self, Self::Error> {
        Ok(data)
    }
}

impl<'a, const N: usize> FixFieldValue<'a> for &'a [u8; N] {
    type Error = ();
    type SerializeSettings = ();

    #[inline(always)]
    fn serialize<B>(&self, buffer: &mut B) -> usize
    where
        B: Buffer,
    {
        buffer.extend_from_slice(&self[..]);
        self.len()
    }

    #[inline(always)]
    fn deserialize(data: &'a [u8]) -> Result<Self, Self::Error> {
        data.try_into().map_err(|_| ())
    }
}

impl<'a> FixFieldValue<'a> for TagU16 {
    type Error = error::Int;
    type SerializeSettings = ();

    #[inline(always)]
    fn serialize<B>(&self, buffer: &mut B) -> usize
    where
        B: Buffer,
    {
        let s = self.to_string();
        buffer.extend_from_slice(s.as_bytes());
        s.len()
    }

    #[inline(always)]
    fn deserialize(data: &'a [u8]) -> Result<Self, Self::Error> {
        let s = std::str::from_utf8(data).map_err(|_| Self::Error::InvalidUtf8)?;
        s.parse().map_err(|_| Self::Error::Other)
    }

    #[inline(always)]
    fn deserialize_lossy(data: &'a [u8]) -> Result<Self, Self::Error> {
        fn ascii_digit_to_u16(digit: u8) -> u16 {
            (digit as u16).wrapping_sub(b'0' as u16)
        }
        let mut n = 0u16;
        for byte in data.iter().copied() {
            n = n.wrapping_mul(10).wrapping_add(ascii_digit_to_u16(byte));
        }
        TagU16::new(n).ok_or(Self::Error::Other)
    }
}

impl<'a> FixFieldValue<'a> for u32 {
    type Error = error::Int;
    type SerializeSettings = Padding;

    #[inline(always)]
    fn serialize<B>(&self, buffer: &mut B) -> usize
    where
        B: Buffer,
    {
        let s = self.to_string();
        buffer.extend_from_slice(s.as_bytes());
        s.len()
    }

    #[inline(always)]
    fn serialize_with<B>(&self, buffer: &mut B, settings: &Self::SerializeSettings) -> usize
    where
        B: Buffer,
    {
        //buffer.extend_from_slice([settings.byte; settings.len]);
        for _ in 0..settings.len {
            // FIXME
            buffer.extend_from_slice(&[((self / 10) + b'0' as u32) as u8]);
        }
        self.serialize(buffer)
    }

    #[inline(always)]
    fn deserialize(data: &'a [u8]) -> Result<Self, Self::Error> {
        let s = std::str::from_utf8(data).map_err(|_| Self::Error::InvalidUtf8)?;
        s.parse().map_err(|_| Self::Error::Other)
    }

    #[inline(always)]
    fn deserialize_lossy(data: &'a [u8]) -> Result<Self, Self::Error> {
        fn ascii_digit_to_u32(digit: u8) -> u32 {
            (digit as u32).wrapping_sub(b'0' as u32)
        }
        let mut n = 0u32;
        for byte in data.iter().copied() {
            n = n.wrapping_mul(10).wrapping_add(ascii_digit_to_u32(byte));
        }
        Ok(n)
    }
}

impl<'a> FixFieldValue<'a> for i32 {
    type Error = error::Int;
    type SerializeSettings = ();

    #[inline(always)]
    fn serialize<B>(&self, buffer: &mut B) -> usize
    where
        B: Buffer,
    {
        let s = self.to_string();
        buffer.extend_from_slice(s.as_bytes());
        s.len()
    }

    #[inline(always)]
    fn deserialize(data: &'a [u8]) -> Result<Self, Self::Error> {
        let s = std::str::from_utf8(data).map_err(|_| Self::Error::InvalidUtf8)?;
        s.parse().map_err(|_| Self::Error::Other)
    }

    #[inline(always)]
    fn deserialize_lossy(data: &'a [u8]) -> Result<Self, Self::Error> {
        fn ascii_digit_to_i32(digit: u8) -> i32 {
            digit as i32 - b'0' as i32
        }
        let mut n = 0;
        for byte in data.iter().copied() {
            n = n * 10 + ascii_digit_to_i32(byte);
        }
        let sign = if data[0] == b'-' { -1 } else { 1 };
        Ok(n * sign)
    }
}

impl<'a> FixFieldValue<'a> for u64 {
    type Error = error::Int;
    type SerializeSettings = ();

    #[inline(always)]
    fn serialize<B>(&self, buffer: &mut B) -> usize
    where
        B: Buffer,
    {
        let s = self.to_string();
        buffer.extend_from_slice(s.as_bytes());
        s.len()
    }

    #[inline(always)]
    fn deserialize(data: &'a [u8]) -> Result<Self, Self::Error> {
        let s = std::str::from_utf8(data).map_err(|_| Self::Error::InvalidUtf8)?;
        s.parse().map_err(|_| Self::Error::Other)
    }

    #[inline(always)]
    fn deserialize_lossy(data: &'a [u8]) -> Result<Self, Self::Error> {
        fn ascii_digit_to_u64(digit: u8) -> u64 {
            digit as u64 - b'0' as u64
        }
        let mut n = 0;
        for byte in data.iter().copied() {
            n = n * 10 + ascii_digit_to_u64(byte);
        }
        Ok(n)
    }
}

impl<'a> FixFieldValue<'a> for i64 {
    type Error = error::Int;
    type SerializeSettings = ();

    #[inline(always)]
    fn serialize<B>(&self, buffer: &mut B) -> usize
    where
        B: Buffer,
    {
        let s = self.to_string();
        buffer.extend_from_slice(s.as_bytes());
        s.len()
    }

    #[inline(always)]
    fn deserialize(data: &'a [u8]) -> Result<Self, Self::Error> {
        let s = std::str::from_utf8(data).map_err(|_| Self::Error::InvalidUtf8)?;
        s.parse().map_err(|_| Self::Error::Other)
    }

    #[inline(always)]
    fn deserialize_lossy(data: &'a [u8]) -> Result<Self, Self::Error> {
        fn ascii_digit_to_i64(digit: u8) -> i64 {
            digit as i64 - b'0' as i64
        }
        let mut n = 0;
        for byte in data.iter().copied() {
            n = n * 10 + ascii_digit_to_i64(byte);
        }
        let sign = if data[0] == b'-' { -1 } else { 1 };
        Ok(n * sign)
    }
}

impl<'a> FixFieldValue<'a> for usize {
    type Error = error::Int;
    type SerializeSettings = ();

    #[inline(always)]
    fn serialize<B>(&self, buffer: &mut B) -> usize
    where
        B: Buffer,
    {
        let s = self.to_string();
        buffer.extend_from_slice(s.as_bytes());
        s.len()
    }

    #[inline(always)]
    fn deserialize(data: &'a [u8]) -> Result<Self, Self::Error> {
        let s = std::str::from_utf8(data).map_err(|_| Self::Error::InvalidUtf8)?;
        s.parse().map_err(|_| Self::Error::Other)
    }

    #[inline(always)]
    fn deserialize_lossy(data: &'a [u8]) -> Result<Self, Self::Error> {
        fn ascii_digit_to_usize(digit: u8) -> usize {
            digit as usize - b'0' as usize
        }
        let mut n = 0;
        for byte in data.iter().copied() {
            n = n * 10 + ascii_digit_to_usize(byte);
        }
        Ok(n)
    }
}

pub trait SuperDataType<'a, T>
where
    Self: FixFieldValue<'a>,
    T: FixFieldValue<'a>,
{
}

/// A [`DataType`] is always a [`SuperDataType`] of itself.
impl<'a, T> SuperDataType<'a, T> for T where T: FixFieldValue<'a> {}

impl<'a> SuperDataType<'a, &'a str> for &'a [u8] {}
impl<'a> SuperDataType<'a, i64> for &'a [u8] {}
impl<'a> SuperDataType<'a, u32> for u64 {}
impl<'a> SuperDataType<'a, i32> for i64 {}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn serialize_bools() {
        let mut buffer = Vec::new();
        assert_eq!(true.serialize(&mut buffer), 1);
        assert_eq!(false.serialize(&mut buffer), 1);
        assert_eq!(&buffer[..], b"YN" as &[u8]);
    }

    #[test]
    fn serialize_bytes() {
        let data: &[&[u8]] = &[b"hello", b"", b" ", b"foo"];
        let mut buffer = Vec::new();
        for slice in data {
            assert_eq!(slice.serialize(&mut buffer), slice.len());
        }
        assert_eq!(&buffer[..], b"hello foo" as &[u8]);
    }

    #[test]
    fn serialize_country() {
        let mut buffer = Vec::new();
        assert_eq!(b"IT".serialize(&mut buffer), 2);
        assert_eq!(&buffer[..], b"IT" as &[u8]);
    }

    #[test]
    fn serialize_currency() {
        let mut buffer = Vec::new();
        assert_eq!(b"USD".serialize(&mut buffer), 3);
        assert_eq!(&buffer[..], b"USD" as &[u8]);
    }
}