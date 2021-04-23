use super::{Config, Configure, FvWrite};
use crate::buffer::Buffer;
use crate::dtf::{CheckSum, DataField};
use crate::definitions::fixt11;
use crate::FieldDef;
use crate::TagU16;
use std::ops::Range;

/// A buffered, content-agnostic FIX encoder.
///
/// [`RawEncoder`] is the fundamental building block for building higher-level
/// FIX encoders. It allows for encoding of arbitrary payloads and takes care of
/// `BodyLength (9)` and `CheckSum (10)`.
///
/// # Examples
///
/// ```
/// use fefix::tagvalue::{Config, RawEncoder};
///
/// let encoder = &mut RawEncoder::<_, Config>::from_buffer(Vec::new());
/// encoder.config_mut().set_separator(b'|');
/// encoder.set_begin_string(b"FIX.4.4");
/// encoder.extend_from_slice(b"35=0|49=A|56=B|34=12|52=20100304-07:59:30|");
/// let data = encoder.finalize();
/// assert_eq!(data, b"8=FIX.4.4|9=000042|35=0|49=A|56=B|34=12|52=20100304-07:59:30|10=216|");
/// ```
#[derive(Debug, Clone)]
pub struct Encoder<B = Vec<u8>, C = Config>
where
    B: Buffer,
    C: Configure,
{
    buffer: B,
    config: C,
}

impl<B, C> Encoder<B, C>
where
    B: Buffer,
    C: Configure,
{
    pub fn from_buffer(buffer: B) -> Self {
        Self {
            buffer,
            config: C::default(),
        }
    }

    pub fn buffer(&self) -> &B {
        &self.buffer
    }

    pub fn buffer_mut(&self) -> &B {
        &self.buffer
    }

    /// Returns an immutable reference to the [`Configure`] implementor used by
    /// `self`.
    pub fn config(&self) -> &C {
        &self.config
    }

    /// Returns a mutable reference to the [`Configure`] implementor used by
    /// `self`.
    pub fn config_mut(&mut self) -> &mut C {
        &mut self.config
    }

    pub fn start_message<'a>(
        &'a mut self,
        begin_string: &'a [u8],
        msg_type: &'a [u8],
    ) -> EncoderHandle<'a, B, C> {
        self.buffer.clear();
        let mut state = EncoderHandle {
            raw_encoder: self,
            body_start_i: 0,
        };
        state.set(fixt11::BEGIN_STRING, begin_string);
        // The second field is supposed to be `BodyLength(9)`, but obviously
        // the length of the message is unknow until later in the
        // serialization phase. This alone would usually require to
        //
        //  1. Serialize the rest of the message into an external buffer.
        //  2. Calculate the length of the message.
        //  3. Serialize `BodyLength(9)` to `buffer`.
        //  4. Copy the contents of the external buffer into `buffer`.
        //  5. ... go on with the serialization process.
        //
        // Luckily, FIX allows for zero-padded integer values and we can
        // leverage this to reserve some space for the value. We waste
        // some bytes but the benefits largely outweight the costs.
        //
        // Six digits (~1MB) ought to be enough for every message.
        state.set_any(fixt11::BODY_LENGTH.tag(), b"000000" as &[u8]);
        state.body_start_i = state.raw_encoder.buffer.len();
        state.set_any(fixt11::MSG_TYPE.tag(), msg_type);
        state
    }
}

/// A type returned by [`Encoder::start_message`](Encoder::start_message) to
/// actually encode data fields.
#[derive(Debug)]
pub struct EncoderHandle<'a, B = Vec<u8>, C = Config>
where
    B: Buffer,
    C: Configure,
{
    raw_encoder: &'a mut Encoder<B, C>,
    body_start_i: usize,
}

impl<'a, B, C> EncoderHandle<'a, B, C>
where
    B: Buffer,
    C: Configure,
{
    /// Adds a `field` with a `value` to the current message.
    pub fn set<'b, T>(&mut self, field: &FieldDef<'b, T>, value: T)
    where
        T: DataField<'b>,
    {
        self.set_any(field.tag(), value)
    }

    pub fn set_any<'b, T>(&mut self, tag: TagU16, value: T)
    where
        T: DataField<'b>,
    {
        tag.serialize(&mut self.raw_encoder.buffer);
        self.raw_encoder.buffer.extend_from_slice(b"=" as &[u8]);
        value.serialize(&mut self.raw_encoder.buffer);
        self.raw_encoder
            .buffer
            .extend_from_slice(&[self.raw_encoder.config().separator()]);
    }

    pub fn raw(&mut self, raw: &[u8]) {
        self.raw_encoder.buffer.extend_from_slice(raw);
    }

    /// Closes the current message writing operation and returns its byte
    /// representation.
    pub fn wrap(mut self) -> &'a [u8] {
        self.write_body_length();
        self.write_checksum();
        self.raw_encoder.buffer.as_slice()
    }

    fn body_length_writable_range(&self) -> Range<usize> {
        self.body_start_i - 7..self.body_start_i - 1
    }

    fn body_length(&self) -> usize {
        self.raw_encoder.buffer.as_slice().len() - self.body_start_i
    }

    fn write_body_length(&mut self) {
        let body_length = self.body_length();
        let body_length_range = self.body_length_writable_range();
        let slice = &mut self.raw_encoder.buffer.as_mut_slice()[body_length_range];
        slice[0] = to_digit((body_length / 100000) as u8 % 10);
        slice[1] = to_digit((body_length / 10000) as u8 % 10);
        slice[2] = to_digit((body_length / 1000) as u8 % 10);
        slice[3] = to_digit((body_length / 100) as u8 % 10);
        slice[4] = to_digit((body_length / 10) as u8 % 10);
        slice[5] = to_digit((body_length / 1) as u8 % 10);
    }

    fn write_checksum(&mut self) {
        let checksum = CheckSum::compute(self.raw_encoder.buffer.as_slice());
        self.set(fixt11::CHECK_SUM, checksum);
    }
}

impl<'a, B, C> FvWrite<'a> for EncoderHandle<'a, B, C>
where
    B: Buffer,
    C: Configure,
{
    type Key = TagU16;

    fn set_fv_with_key<'b, T>(&'b mut self, key: &Self::Key, value: T)
    where
        T: DataField<'b>,
    {
        self.set_any(*key, value);
    }

    fn set_fv<'b, T, S>(&'b mut self, field: &FieldDef<'b, T>, value: S)
    where
        T: DataField<'b>,
        S: DataField<'b>,
    {
        self.set_fv_with_key(&field.tag(), value);
    }
}

fn to_digit(byte: u8) -> u8 {
    byte + b'0'
}
