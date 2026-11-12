//! S2: canonical source bytes only, not projection meaning or admission.
//! No source tree or complete key is retained. Adjacent decoded keys are compared
//! by bounded rereads, separately charged from the one-pass main digest.

use super::bytes::{
    BoundColumn, ByteUsage, RawColumn, SourceIdentity, FRAGMENT_BYTES, INPUT_BYTES,
};
use super::{data_version, preflight_schema_and_control_rows, verify_schema_v8};
use crate::SidecarError;
use rusqlite::{Connection, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::io::{self, Write};

const MAX_DEPTH: usize = 127;
const CHUNK_BYTES: usize = 64 * 1024;
// Locked serde_json 1.0.150 uses u64/i64/finite f64 (no arbitrary_precision).
// Its serializer delegates f64 output to zmij 1.0.23's [MaybeUninit<u8>; 24].
// Integers need at most 20 bytes. A longer candidate cannot be in this output
// image, even when serde can parse/round it. Guarded by differential tests.
const NUMBER_BYTES: usize = 24;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct CanonicalUsage {
    pub(crate) input: ByteUsage,
    pub(crate) main_read_bytes: usize,
    pub(crate) key_read_bytes: usize,
    pub(crate) main_consumed_bytes: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CanonicalColumnStep {
    Pending(CanonicalUsage),
    Cancelled,
    CanonicalComplete {
        byte_length: u64,
        sha256: [u8; 32],
        usage: CanonicalUsage,
    },
}

pub(super) struct Progress {
    bound: BoundColumn,
    after: u64,
    hash: Sha256,
    parser: Parser,
    comparison: Option<KeyComparison>,
}
pub(super) type ScanPosition = (u64, Option<(u64, u64)>);

pub(crate) struct CanonicalColumnSession<'connection> {
    connection: &'connection mut Connection,
    epoch: i64,
    progress: Option<Progress>,
    #[cfg(test)]
    start_usage: CanonicalUsage,
    #[cfg(test)]
    last_usage: CanonicalUsage,
    #[cfg(test)]
    fragment_limit: usize,
}

pub(super) enum Disposition {
    Pending,
    Complete,
    Cancelled,
}

impl<'connection> CanonicalColumnSession<'connection> {
    pub(crate) fn start(
        connection: &'connection mut Connection,
        column: RawColumn,
        expected: SourceIdentity,
    ) -> Result<Self, SidecarError> {
        if !connection.is_autocommit() || expected.source_rowid <= 0 {
            return Err(invalid());
        }
        let mut usage = CanonicalUsage::default();
        usage.input.retained_bytes = expected.bytes()?;
        if usage.input.retained_bytes > INPUT_BYTES {
            return Err(invalid());
        }
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| invalid())?;
        let epoch = data_version(&tx)?;
        // Same existing bootstrap as S1, under the initial lock, separately
        // accounted from source input. It does not read the source column.
        preflight_schema_and_control_rows(&tx)?;
        verify_schema_v8(&tx)?;
        let bound = BoundColumn::bind(&tx, column, expected, &mut usage.input)?;
        tx.commit().map_err(|_| invalid())?;
        if !connection.is_autocommit() {
            return Err(invalid());
        }
        Ok(Self {
            connection,
            epoch,
            progress: Some(Progress {
                bound,
                after: 0,
                hash: Sha256::new(),
                parser: Parser::default(),
                comparison: None,
            }),
            #[cfg(test)]
            start_usage: usage,
            #[cfg(test)]
            last_usage: CanonicalUsage::default(),
            #[cfg(test)]
            fragment_limit: CHUNK_BYTES,
        })
    }

    pub(crate) fn step(
        &mut self,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<CanonicalColumnStep, SidecarError> {
        let mut progress = self.progress.take().ok_or_else(invalid)?;
        #[cfg(test)]
        {
            self.last_usage = CanonicalUsage::default();
        }
        if !self.connection.is_autocommit() {
            return Err(invalid());
        }
        let mut usage = CanonicalUsage::default();
        usage.input.retained_bytes = progress
            .bound
            .retained_bytes()?
            .checked_add(progress.parser.retained_bytes())
            .and_then(|n| {
                n.checked_add(
                    progress
                        .comparison
                        .as_ref()
                        .map_or(0, KeyComparison::retained_bytes),
                )
            })
            .ok_or_else(invalid)?;
        if usage.input.retained_bytes > INPUT_BYTES {
            return Err(invalid());
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| invalid())?;
        #[cfg(test)]
        let fragment_limit = self.fragment_limit;
        #[cfg(not(test))]
        let fragment_limit = CHUNK_BYTES;
        let checked = (|| {
            if data_version(&tx)? != self.epoch {
                return Err(invalid());
            }
            if cancelled() {
                return Ok(Disposition::Cancelled);
            }
            progress.bound.recheck(&tx, &mut usage.input)?;
            let result = progress.scan(&tx, &mut usage, fragment_limit, cancelled)?;
            if matches!(result, Disposition::Cancelled) {
                return Ok(result);
            }
            if cancelled() {
                return Ok(Disposition::Cancelled);
            }
            Ok(result)
        })();
        #[cfg(test)]
        {
            self.last_usage = CanonicalUsage {
                input: ByteUsage {
                    retained_bytes: usage.input.retained_bytes,
                    metadata_bytes: usage.input.metadata_bytes,
                    fragment_bytes: usage.input.fragment_bytes,
                    work_units: usage.input.work_units,
                },
                main_read_bytes: usage.main_read_bytes,
                key_read_bytes: usage.key_read_bytes,
                main_consumed_bytes: usage.main_consumed_bytes,
            };
        }
        match checked {
            Ok(Disposition::Cancelled) => {
                tx.rollback().map_err(|_| invalid())?;
                Ok(CanonicalColumnStep::Cancelled)
            }
            Err(error) => {
                tx.rollback().map_err(|_| invalid())?;
                Err(error)
            }
            Ok(result) => {
                tx.commit().map_err(|_| invalid())?;
                if !self.connection.is_autocommit() {
                    return Err(invalid());
                }
                match result {
                    Disposition::Complete => Ok(CanonicalColumnStep::CanonicalComplete {
                        byte_length: progress.bound.byte_length,
                        sha256: progress.hash.finalize().into(),
                        usage,
                    }),
                    Disposition::Pending => {
                        self.progress = Some(progress);
                        Ok(CanonicalColumnStep::Pending(usage))
                    }
                    Disposition::Cancelled => unreachable!(),
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn usage_for_test(
        &self,
    ) -> (&CanonicalUsage, &CanonicalUsage, Option<ScanPosition>) {
        (
            &self.start_usage,
            &self.last_usage,
            self.progress.as_ref().map(Progress::position),
        )
    }
    #[cfg(test)]
    pub(crate) fn autocommit_for_test(&self) -> bool {
        self.connection.is_autocommit()
    }
    #[cfg(test)]
    pub(crate) fn fragment_limit_for_test(&mut self, limit: usize) {
        assert!((1..=CHUNK_BYTES).contains(&limit));
        self.fragment_limit = limit;
    }
}

fn invalid() -> SidecarError {
    SidecarError::new(
        "storageSchemaMismatch",
        "source column is not valid canonical JSON",
    )
}

fn available(usage: &CanonicalUsage) -> Result<usize, SidecarError> {
    Ok(FRAGMENT_BYTES
        .checked_sub(usage.input.fragment_bytes)
        .ok_or_else(invalid)?
        .min(
            INPUT_BYTES
                .checked_sub(usage.input.encoded_bytes())
                .ok_or_else(invalid)?,
        ))
}

impl Progress {
    pub(super) fn new(bound: BoundColumn) -> Self {
        Self {
            bound,
            after: 0,
            hash: Sha256::new(),
            parser: Parser::default(),
            comparison: None,
        }
    }
    pub(super) fn bound(&self) -> &BoundColumn {
        &self.bound
    }
    pub(super) fn into_complete(mut self) -> Result<(BoundColumn, [u8; 32]), SidecarError> {
        if self.after != self.bound.byte_length || self.comparison.is_some() {
            return Err(invalid());
        }
        self.parser.finish()?;
        Ok((self.bound, self.hash.finalize().into()))
    }
    pub(super) fn retained_bytes(&self) -> Result<usize, SidecarError> {
        self.bound
            .retained_bytes()?
            .checked_add(self.parser.retained_bytes())
            .and_then(|n| {
                n.checked_add(
                    self.comparison
                        .as_ref()
                        .map_or(0, KeyComparison::retained_bytes),
                )
            })
            .ok_or_else(invalid)
    }
    pub(super) fn position(&self) -> ScanPosition {
        (
            self.after,
            self.comparison
                .as_ref()
                .map(|c| (c.left.after, c.right.after)),
        )
    }

    pub(super) fn scan(
        &mut self,
        connection: &Connection,
        usage: &mut CanonicalUsage,
        limit: usize,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Disposition, SidecarError> {
        let initial = self.position();
        loop {
            if self.comparison.is_none() && self.after == self.bound.byte_length {
                self.parser.finish()?;
                return Ok(Disposition::Complete);
            }
            if usage.input.work_units >= 100 || available(usage)? == 0 {
                break;
            }
            if let Some(comparison) = self.comparison.as_mut() {
                // A comparison candidate plus at most two indexed reads.
                if usage.input.work_units > 97 {
                    break;
                }
                match comparison.advance(&self.bound, connection, usage, limit, cancelled)? {
                    Disposition::Complete => self.comparison = None,
                    Disposition::Pending => {}
                    Disposition::Cancelled => return Ok(Disposition::Cancelled),
                }
                continue;
            }
            let amount = (self.bound.byte_length - self.after)
                .min(limit.min(available(usage)?) as u64) as usize;
            let fragment =
                self.bound
                    .read_span(connection, self.after, amount, &mut usage.input)?;
            usage.main_read_bytes += fragment.len();
            if cancelled() {
                return Ok(Disposition::Cancelled);
            }
            let mut consumed = 0;
            while consumed < fragment.len() {
                let Some(parsed) = self.parser.consume(
                    fragment[consumed],
                    self.after + consumed as u64,
                    &mut usage.input,
                )?
                else {
                    break;
                };
                if parsed.consumed {
                    consumed += 1;
                }
                if let Some((left, right)) = parsed.keys {
                    self.comparison = Some(KeyComparison::new(left, right));
                    break;
                }
            }
            // Unconsumed suffix is deliberately discarded and charged again
            // if reread. Only the main cursor's consumed prefix enters SHA.
            self.hash.update(&fragment[..consumed]);
            self.after = self
                .after
                .checked_add(consumed as u64)
                .ok_or_else(invalid)?;
            usage.main_consumed_bytes += consumed;
        }
        if self.position() == initial {
            return Err(invalid());
        }
        Ok(Disposition::Pending)
    }
}

#[derive(Clone, Copy)]
struct Span {
    start: u64,
    end: u64,
}
#[derive(Clone, Copy)]
enum ArrayState {
    First,
    Value,
    Comma,
}
#[derive(Clone, Copy)]
enum ObjectState {
    First,
    Key,
    Colon,
    Value,
    Comma,
}
enum Frame {
    Array(ArrayState),
    Object {
        state: ObjectState,
        previous: Option<Span>,
    },
}

#[derive(Default)]
struct Parser {
    frames: Vec<Frame>, // capped at MAX_DEPTH before every push
    started: bool,
    token: Option<Token>,
}

enum Token {
    String(StringToken),
    Literal {
        word: &'static [u8],
        after: usize,
    },
    Number {
        bytes: [u8; NUMBER_BYTES],
        len: usize,
    },
}
struct Parsed {
    consumed: bool,
    keys: Option<(Span, Span)>,
}

impl Parser {
    fn retained_bytes(&self) -> usize {
        match self.token.as_ref() {
            Some(Token::String(s)) => s.escape.retained_bytes() + s.utf8.len,
            Some(Token::Number { len, .. }) => *len,
            _ => 0, // literal identity and index are fixed grammar state
        }
    }
    fn consume(
        &mut self,
        byte: u8,
        offset: u64,
        usage: &mut ByteUsage,
    ) -> Result<Option<Parsed>, SidecarError> {
        let mut keys = None;
        if let Some(mut token) = self.token.take() {
            let mut done = false;
            match &mut token {
                Token::String(string) => {
                    if string.accept(byte)? {
                        done = true;
                        if string.key {
                            let span = Span {
                                start: string.start,
                                end: offset,
                            };
                            match self.frames.last_mut() {
                                Some(Frame::Object { state, previous }) => {
                                    keys = previous.map(|left| (left, span));
                                    *previous = Some(span);
                                    *state = ObjectState::Colon;
                                }
                                _ => return Err(invalid()),
                            }
                        }
                    }
                }
                Token::Literal { word, after } => {
                    if word.get(*after) != Some(&byte) {
                        return Err(invalid());
                    }
                    *after += 1;
                    done = *after == word.len();
                }
                Token::Number { bytes, len } => {
                    if matches!(byte, b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E') {
                        if *len == NUMBER_BYTES {
                            return Err(invalid());
                        }
                        bytes[*len] = byte;
                        *len += 1;
                    } else {
                        canonical_number(&bytes[..*len])?;
                        return Ok(Some(Parsed {
                            consumed: false,
                            keys: None,
                        }));
                    }
                }
            }
            if !done {
                self.token = Some(token);
            }
            return Ok(Some(Parsed {
                consumed: true,
                keys,
            }));
        }
        if usage.work_units >= 100 {
            return Ok(None);
        }
        usage.work_units += 1;
        match self.frames.last_mut() {
            Some(Frame::Array(state)) => match state {
                ArrayState::First if byte == b']' => {
                    self.frames.pop();
                    return Ok(Some(Parsed {
                        consumed: true,
                        keys: None,
                    }));
                }
                ArrayState::Comma => {
                    match byte {
                        b',' => *state = ArrayState::Value,
                        b']' => {
                            self.frames.pop();
                        }
                        _ => return Err(invalid()),
                    }
                    return Ok(Some(Parsed {
                        consumed: true,
                        keys: None,
                    }));
                }
                _ => *state = ArrayState::Comma,
            },
            Some(Frame::Object { state, .. }) => match state {
                ObjectState::First if byte == b'}' => {
                    self.frames.pop();
                    return Ok(Some(Parsed {
                        consumed: true,
                        keys: None,
                    }));
                }
                ObjectState::First | ObjectState::Key => {
                    if byte != b'"' {
                        return Err(invalid());
                    }
                    self.token = Some(Token::String(StringToken::new(true, offset + 1)));
                    return Ok(Some(Parsed {
                        consumed: true,
                        keys: None,
                    }));
                }
                ObjectState::Colon => {
                    if byte != b':' {
                        return Err(invalid());
                    }
                    *state = ObjectState::Value;
                    return Ok(Some(Parsed {
                        consumed: true,
                        keys: None,
                    }));
                }
                ObjectState::Comma => {
                    match byte {
                        b',' => *state = ObjectState::Key,
                        b'}' => {
                            self.frames.pop();
                        }
                        _ => return Err(invalid()),
                    }
                    return Ok(Some(Parsed {
                        consumed: true,
                        keys: None,
                    }));
                }
                ObjectState::Value => *state = ObjectState::Comma,
            },
            None if self.started => return Err(invalid()),
            None => self.started = true,
        }
        match byte {
            b'[' | b'{' => {
                if self.frames.len() == MAX_DEPTH {
                    return Err(invalid());
                }
                self.frames.push(if byte == b'[' {
                    Frame::Array(ArrayState::First)
                } else {
                    Frame::Object {
                        state: ObjectState::First,
                        previous: None,
                    }
                });
            }
            b'"' => self.token = Some(Token::String(StringToken::new(false, offset + 1))),
            b't' | b'f' | b'n' => {
                self.token = Some(Token::Literal {
                    word: match byte {
                        b't' => b"true",
                        b'f' => b"false",
                        _ => b"null",
                    },
                    after: 1,
                })
            }
            b'-' | b'0'..=b'9' => {
                let mut bytes = [0; NUMBER_BYTES];
                bytes[0] = byte;
                self.token = Some(Token::Number { bytes, len: 1 });
            }
            _ => return Err(invalid()),
        }
        Ok(Some(Parsed {
            consumed: true,
            keys: None,
        }))
    }
    fn finish(&mut self) -> Result<(), SidecarError> {
        match self.token.take() {
            Some(Token::Number { bytes, len }) => canonical_number(&bytes[..len])?,
            Some(_) => return Err(invalid()),
            None => {}
        }
        if !self.started || !self.frames.is_empty() {
            return Err(invalid());
        }
        Ok(())
    }
}

struct NumberOutput {
    bytes: [u8; NUMBER_BYTES],
    len: usize,
}
impl Write for NumberOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let end = self
            .len
            .checked_add(bytes.len())
            .filter(|n| *n <= NUMBER_BYTES)
            .ok_or_else(|| io::Error::other("number output bound changed"))?;
        self.bytes[self.len..end].copy_from_slice(bytes);
        self.len = end;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn canonical_number(bytes: &[u8]) -> Result<(), SidecarError> {
    let number: serde_json::Number = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let mut output = NumberOutput {
        bytes: [0; NUMBER_BYTES],
        len: 0,
    };
    serde_json::to_writer(&mut output, &number).map_err(|_| invalid())?;
    if bytes != &output.bytes[..output.len] {
        return Err(invalid());
    }
    Ok(())
}

#[derive(Default)]
enum Escape {
    #[default]
    Plain,
    Slash,
    Unicode {
        value: u8,
        digits: u8,
    },
}
impl Escape {
    fn retained_bytes(&self) -> usize {
        match self {
            Self::Plain => 0,
            Self::Slash => 1,
            Self::Unicode { digits, .. } => 2 + usize::from(*digits),
        }
    }
    fn decode(&mut self, byte: u8) -> Result<Option<u8>, SidecarError> {
        let decoded = match self {
            Self::Plain => match byte {
                b'\\' => {
                    *self = Self::Slash;
                    return Ok(None);
                }
                0..=31 | b'"' => return Err(invalid()),
                _ => return Ok(Some(byte)),
            },
            Self::Slash => match byte {
                b'"' | b'\\' => byte,
                b'b' => 8,
                b't' => 9,
                b'n' => 10,
                b'f' => 12,
                b'r' => 13,
                b'u' => {
                    *self = Self::Unicode {
                        value: 0,
                        digits: 0,
                    };
                    return Ok(None);
                }
                _ => return Err(invalid()),
            },
            Self::Unicode { value, digits } => {
                let hex = match byte {
                    b'0'..=b'9' => byte - b'0',
                    b'a'..=b'f' => byte - b'a' + 10,
                    _ => return Err(invalid()),
                };
                *value = value
                    .checked_mul(16)
                    .and_then(|n| n.checked_add(hex))
                    .ok_or_else(invalid)?;
                *digits += 1;
                if *digits < 4 {
                    return Ok(None);
                }
                if *value > 31 || matches!(*value, 8 | 9 | 10 | 12 | 13) {
                    return Err(invalid());
                }
                *value
            }
        };
        *self = Self::Plain;
        Ok(Some(decoded))
    }
}

#[derive(Default)]
struct Utf8 {
    bytes: [u8; 4],
    len: usize,
}
impl Utf8 {
    fn accept(&mut self, byte: u8) -> Result<(), SidecarError> {
        if self.len == 0 && byte < 128 {
            return Ok(());
        }
        if self.len == self.bytes.len() {
            return Err(invalid());
        }
        self.bytes[self.len] = byte;
        self.len += 1;
        match std::str::from_utf8(&self.bytes[..self.len]) {
            Ok(_) => self.len = 0,
            Err(error) if error.error_len().is_none() => {}
            Err(_) => return Err(invalid()),
        }
        Ok(())
    }
}
struct StringToken {
    key: bool,
    start: u64,
    escape: Escape,
    utf8: Utf8,
}
impl StringToken {
    fn new(key: bool, start: u64) -> Self {
        Self {
            key,
            start,
            escape: Escape::Plain,
            utf8: Utf8::default(),
        }
    }
    fn accept(&mut self, byte: u8) -> Result<bool, SidecarError> {
        if matches!(self.escape, Escape::Plain) && byte == b'"' {
            if self.utf8.len != 0 {
                return Err(invalid());
            }
            return Ok(true);
        }
        if let Some(decoded) = self.escape.decode(byte)? {
            self.utf8.accept(decoded)?;
        }
        Ok(false)
    }
}

struct KeyCursor {
    after: u64,
    end: u64,
    escape: Escape,
}
impl KeyCursor {
    fn next(&mut self, bytes: &[u8], index: &mut usize) -> Result<Option<u8>, SidecarError> {
        while *index < bytes.len() {
            let byte = bytes[*index];
            *index += 1;
            self.after = self.after.checked_add(1).ok_or_else(invalid)?;
            if let Some(decoded) = self.escape.decode(byte)? {
                return Ok(Some(decoded));
            }
        }
        if self.after == self.end && !matches!(self.escape, Escape::Plain) {
            return Err(invalid());
        }
        Ok(None)
    }
}
struct KeyComparison {
    left: KeyCursor,
    right: KeyCursor,
    left_byte: Option<u8>,
    right_byte: Option<u8>,
}
impl KeyComparison {
    fn new(left: Span, right: Span) -> Self {
        Self {
            left: KeyCursor {
                after: left.start,
                end: left.end,
                escape: Escape::Plain,
            },
            right: KeyCursor {
                after: right.start,
                end: right.end,
                escape: Escape::Plain,
            },
            left_byte: None,
            right_byte: None,
        }
    }
    fn retained_bytes(&self) -> usize {
        self.left.escape.retained_bytes()
            + self.right.escape.retained_bytes()
            + usize::from(self.left_byte.is_some())
            + usize::from(self.right_byte.is_some())
    }
    fn advance(
        &mut self,
        bound: &BoundColumn,
        connection: &Connection,
        usage: &mut CanonicalUsage,
        limit: usize,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Disposition, SidecarError> {
        usage.input.work_units += 1;
        let room = available(usage)?;
        let left_cap = if room == 1 && self.left_byte.is_some() && self.right_byte.is_none() {
            // Spend the last byte on the needed RHS, not a speculative LHS
            // suffix behind an already decoded pending byte.
            0
        } else if self.left.after < self.left.end && self.right.after < self.right.end {
            room.div_ceil(2)
        } else {
            room
        };
        let mut read = |cursor: &KeyCursor, cap: usize| -> Result<Vec<u8>, SidecarError> {
            // A pending decoded byte precedes this cursor. Read its following
            // span too, so consuming the byte does not strand the other side's
            // whole buffer and create alternating one-byte reread progress.
            if cursor.after == cursor.end {
                return Ok(Vec::new());
            }
            let amount = (cursor.end - cursor.after)
                .min(limit.min(cap).min(available(usage)?) as u64)
                as usize;
            if amount == 0 {
                return Ok(Vec::new());
            }
            let bytes = bound.read_span(connection, cursor.after, amount, &mut usage.input)?;
            usage.key_read_bytes += bytes.len();
            Ok(bytes)
        };
        let left = read(&self.left, left_cap)?;
        if cancelled() {
            return Ok(Disposition::Cancelled);
        }
        let right = read(&self.right, room)?;
        if cancelled() {
            return Ok(Disposition::Cancelled);
        }
        let (mut l, mut r) = (0, 0);
        loop {
            if self.left_byte.is_none() {
                self.left_byte = self.left.next(&left, &mut l)?;
            }
            if self.right_byte.is_none() {
                self.right_byte = self.right.next(&right, &mut r)?;
            }
            match (self.left_byte, self.right_byte) {
                (Some(a), Some(b)) if a < b => return Ok(Disposition::Complete),
                (Some(a), Some(b)) if a > b => return Err(invalid()),
                (Some(_), Some(_)) => {
                    self.left_byte = None;
                    self.right_byte = None;
                }
                (None, Some(_)) if self.left.after == self.left.end => {
                    return Ok(Disposition::Complete)
                }
                (Some(_), None) if self.right.after == self.right.end => return Err(invalid()),
                (None, None)
                    if self.left.after == self.left.end && self.right.after == self.right.end =>
                {
                    return Err(invalid())
                } // duplicate
                _ => return Ok(Disposition::Pending),
            }
        }
    }
}
