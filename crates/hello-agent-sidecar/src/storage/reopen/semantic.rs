//! One source/projection pair, not global coverage, provenance or admission.
//! Whole values and keys are represented only by immutable source spans.

use super::bytes::{
    BoundColumn, ByteUsage, ProjectionBinding, RawColumn, SourceIdentity, FRAGMENT_BYTES,
    INPUT_BYTES,
};
use super::canonical::{CanonicalUsage, Disposition, Progress as CanonicalProgress};
use super::{data_version, preflight_schema_and_control_rows, verify_schema_v8};
use crate::SidecarError;
use rusqlite::{Connection, TransactionBehavior};
use std::io::Write;

const CHUNK: usize = 64 * 1024;
const DEPTH: usize = 127;
const SEQUENCE: &[u8] = b"last_event_sequence";
fn invalid() -> SidecarError {
    SidecarError::new(
        "storageSchemaMismatch",
        "source/projection compatibility is invalid",
    )
}

enum Poll<T> {
    Pending,
    Ready(T),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Source,
    Projection,
}
#[derive(Clone, Copy, Debug)]
struct Span {
    side: Side,
    start: u64,
    end: u64,
}
impl Span {
    fn content(self) -> Self {
        Self {
            start: self.start + 1,
            end: self.end - 1,
            ..self
        }
    }
}
struct Cache {
    start: u64,
    bytes: Vec<u8>,
}
struct Input<'a> {
    connection: &'a Connection,
    source: &'a BoundColumn,
    projection: &'a ProjectionBinding,
    usage: &'a mut ByteUsage,
    cache: [Option<Cache>; 2],
    progress: u64,
    limit: usize,
    cancelled: &'a mut dyn FnMut() -> bool,
    cancelled_seen: bool,
}
enum Read {
    Byte(u8),
    End,
    Pending,
}
impl Input<'_> {
    fn unit(&mut self) -> bool {
        if (self.cancelled)() {
            self.cancelled_seen = true;
            return false;
        }
        if self.usage.work_units >= 100 {
            false
        } else {
            self.usage.work_units += 1;
            true
        }
    }
    fn at(&mut self, span: Span, after: u64) -> Result<Read, SidecarError> {
        if after == span.end {
            return Ok(Read::End);
        }
        if after > span.end {
            return Err(invalid());
        }
        let slot = match span.side {
            Side::Source => 0,
            Side::Projection => 1,
        };
        if let Some(cache) = &self.cache[slot] {
            if let Some(byte) = after
                .checked_sub(cache.start)
                .and_then(|i| cache.bytes.get(i as usize))
            {
                return Ok(Read::Byte(*byte));
            }
        }
        let room = FRAGMENT_BYTES
            .checked_sub(self.usage.fragment_bytes)
            .ok_or_else(invalid)?
            .min(
                INPUT_BYTES
                    .checked_sub(self.usage.encoded_bytes())
                    .ok_or_else(invalid)?,
            );
        if room == 0 || self.usage.work_units >= 100 {
            return Ok(Read::Pending);
        }
        let amount = (span.end - after).min(room.min(self.limit) as u64) as usize;
        if (self.cancelled)() {
            self.cancelled_seen = true;
            return Ok(Read::Pending);
        }
        let bytes = match span.side {
            Side::Source => self
                .source
                .read_span(self.connection, after, amount, self.usage)?,
            Side::Projection => {
                self.projection
                    .read_span(self.connection, after, amount, self.usage)?
            }
        };
        let byte = *bytes.first().ok_or_else(invalid)?;
        self.cache[slot] = Some(Cache {
            start: after,
            bytes,
        });
        Ok(Read::Byte(byte))
    }
    fn advance(&mut self, after: &mut u64) {
        *after += 1;
        self.progress += 1;
    }
}
fn whitespace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

#[derive(Clone, Copy)]
enum NumberState {
    First,
    FirstDigit,
    Zero,
    Integer,
    FractionFirst,
    Fraction,
    ExponentFirst,
    ExponentDigit,
    Exponent,
}
struct JsonNumber {
    state: NumberState,
    negative: bool,
    significand: u64,
    integer_exponent: i32,
    decimal_exponent: i32,
    exponent: i32,
    negative_exponent: bool,
    exponent_overflow: bool,
    integer_overflow: bool,
    decimal_overflow: bool,
    float: bool,
}
impl Default for JsonNumber {
    fn default() -> Self {
        Self {
            state: NumberState::First,
            negative: false,
            significand: 0,
            integer_exponent: 0,
            decimal_exponent: 0,
            exponent: 0,
            negative_exponent: false,
            exponent_overflow: false,
            integer_overflow: false,
            decimal_overflow: false,
            float: false,
        }
    }
}
impl JsonNumber {
    // Mirrors locked serde_json 1.0.150's non-float_roundtrip parser. No raw
    // numeral is retained; all discarded digits are nevertheless consumed.
    fn accept(&mut self, b: u8) -> Result<bool, SidecarError> {
        let digit = b.is_ascii_digit().then(|| u64::from(b - b'0'));
        match self.state {
            NumberState::First if b == b'-' => {
                self.negative = true;
                self.state = NumberState::FirstDigit;
            }
            NumberState::First | NumberState::FirstDigit => {
                self.significand = digit.ok_or_else(invalid)?;
                self.state = if b == b'0' {
                    NumberState::Zero
                } else {
                    NumberState::Integer
                };
            }
            NumberState::Zero | NumberState::Integer => {
                if let Some(digit) = digit {
                    if matches!(self.state, NumberState::Zero) {
                        return Err(invalid());
                    }
                    if !self.integer_overflow {
                        if let Some(n) = self
                            .significand
                            .checked_mul(10)
                            .and_then(|n| n.checked_add(digit))
                        {
                            self.significand = n;
                            return Ok(true);
                        }
                        self.integer_overflow = true;
                        self.float = true;
                    }
                    // SQLite's maximum column length is below i32::MAX; use
                    // checked arithmetic rather than the upstream unchecked +.
                    self.integer_exponent =
                        self.integer_exponent.checked_add(1).ok_or_else(invalid)?;
                } else if b == b'.' {
                    self.float = true;
                    self.state = NumberState::FractionFirst;
                } else if matches!(b, b'e' | b'E') {
                    self.float = true;
                    self.state = NumberState::ExponentFirst;
                } else {
                    return Ok(false);
                }
            }
            NumberState::FractionFirst | NumberState::Fraction => {
                if let Some(digit) = digit {
                    self.state = NumberState::Fraction;
                    if !self.decimal_overflow {
                        if let Some(n) = self
                            .significand
                            .checked_mul(10)
                            .and_then(|n| n.checked_add(digit))
                        {
                            self.significand = n;
                            self.decimal_exponent =
                                self.decimal_exponent.checked_sub(1).ok_or_else(invalid)?;
                        } else {
                            self.decimal_overflow = true;
                        }
                    }
                } else if matches!(self.state, NumberState::FractionFirst) {
                    return Err(invalid());
                } else if matches!(b, b'e' | b'E') {
                    self.state = NumberState::ExponentFirst;
                } else {
                    return Ok(false);
                }
            }
            NumberState::ExponentFirst if matches!(b, b'+' | b'-') => {
                self.negative_exponent = b == b'-';
                self.state = NumberState::ExponentDigit;
            }
            NumberState::ExponentFirst | NumberState::ExponentDigit | NumberState::Exponent => {
                if let Some(digit) = digit {
                    self.state = NumberState::Exponent;
                    if !self.exponent_overflow {
                        if let Some(n) = self
                            .exponent
                            .checked_mul(10)
                            .and_then(|n| n.checked_add(digit as i32))
                        {
                            self.exponent = n;
                        } else {
                            self.exponent_overflow = true;
                        }
                    }
                } else if matches!(self.state, NumberState::Exponent) {
                    return Ok(false);
                } else {
                    return Err(invalid());
                }
            }
        }
        Ok(true)
    }
    fn finish(&self) -> Result<serde_json::Number, SidecarError> {
        if matches!(
            self.state,
            NumberState::First
                | NumberState::FirstDigit
                | NumberState::FractionFirst
                | NumberState::ExponentFirst
                | NumberState::ExponentDigit
        ) {
            return Err(invalid());
        }
        let mut text = NumberImage {
            bytes: [0; 33],
            len: 0,
        };
        if self.negative {
            text.write_all(b"-").map_err(|_| invalid())?;
        }
        if self.exponent_overflow {
            if self.significand != 0 && !self.negative_exponent {
                return Err(invalid());
            }
            text.write_all(b"0.0").map_err(|_| invalid())?;
        } else {
            write!(text, "{}", self.significand).map_err(|_| invalid())?;
            if self.float {
                let initial = self
                    .integer_exponent
                    .checked_add(self.decimal_exponent)
                    .ok_or_else(invalid)?;
                let exponent = if self.negative_exponent {
                    initial.saturating_sub(self.exponent)
                } else {
                    initial.saturating_add(self.exponent)
                };
                write!(text, "e{exponent}").map_err(|_| invalid())?;
            }
        }
        serde_json::from_slice(&text.bytes[..text.len]).map_err(|_| invalid())
    }
}
struct NumberImage {
    bytes: [u8; 33],
    len: usize,
}
impl Write for NumberImage {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let end = self
            .len
            .checked_add(bytes.len())
            .filter(|n| *n <= 33)
            .ok_or_else(|| std::io::Error::other("number image bound"))?;
        self.bytes[self.len..end].copy_from_slice(bytes);
        self.len = end;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Default)]
enum Escape {
    #[default]
    Plain,
    Slash,
    Hex {
        value: u16,
        digits: u8,
        high: Option<u16>,
    },
    LowSlash(u16),
    LowU(u16),
}
#[derive(Default)]
struct Decoder {
    escape: Escape,
    utf8: [u8; 4],
    utf8_len: usize,
}
impl Decoder {
    fn retained(&self) -> usize {
        self.utf8_len
            + match self.escape {
                Escape::Plain => 0,
                Escape::Slash => 1,
                Escape::Hex { digits, high, .. } => {
                    2 + usize::from(digits) + if high.is_some() { 6 } else { 0 }
                }
                Escape::LowSlash(_) => 6,
                Escape::LowU(_) => 7,
            }
    }
    fn plain(&self) -> bool {
        matches!(self.escape, Escape::Plain) && self.utf8_len == 0
    }
    fn accept(&mut self, b: u8) -> Result<([u8; 4], usize), SidecarError> {
        let decoded = match self.escape {
            Escape::Plain => {
                if b == b'\\' {
                    if self.utf8_len != 0 {
                        return Err(invalid());
                    }
                    self.escape = Escape::Slash;
                    return Ok(([0; 4], 0));
                }
                if b < 32 || b == b'"' {
                    return Err(invalid());
                }
                b
            }
            Escape::Slash => {
                if b == b'u' {
                    self.escape = Escape::Hex {
                        value: 0,
                        digits: 0,
                        high: None,
                    };
                    return Ok(([0; 4], 0));
                }
                self.escape = Escape::Plain;
                return Ok((
                    [
                        match b {
                            b'"' => b'"',
                            b'\\' => b'\\',
                            b'/' => b'/',
                            b'b' => 8,
                            b'f' => 12,
                            b'n' => 10,
                            b'r' => 13,
                            b't' => 9,
                            _ => return Err(invalid()),
                        },
                        0,
                        0,
                        0,
                    ],
                    1,
                ));
            }
            Escape::Hex {
                value,
                digits,
                high,
            } => {
                let digit = match b {
                    b'0'..=b'9' => b - b'0',
                    b'a'..=b'f' => b - b'a' + 10,
                    b'A'..=b'F' => b - b'A' + 10,
                    _ => return Err(invalid()),
                };
                let value = (value << 4) | u16::from(digit);
                if digits < 3 {
                    self.escape = Escape::Hex {
                        value,
                        digits: digits + 1,
                        high,
                    };
                    return Ok(([0; 4], 0));
                }
                if let Some(high) = high {
                    if !(0xdc00..=0xdfff).contains(&value) {
                        return Err(invalid());
                    }
                    self.escape = Escape::Plain;
                    let scalar =
                        0x10000 + ((u32::from(high) - 0xd800) << 10) + (u32::from(value) - 0xdc00);
                    let mut bytes = [0; 4];
                    let len = char::from_u32(scalar)
                        .ok_or_else(invalid)?
                        .encode_utf8(&mut bytes)
                        .len();
                    return Ok((bytes, len));
                }
                if (0xd800..=0xdbff).contains(&value) {
                    self.escape = Escape::LowSlash(value);
                    return Ok(([0; 4], 0));
                }
                if (0xdc00..=0xdfff).contains(&value) {
                    return Err(invalid());
                }
                self.escape = Escape::Plain;
                let mut bytes = [0; 4];
                let len = char::from_u32(u32::from(value))
                    .ok_or_else(invalid)?
                    .encode_utf8(&mut bytes)
                    .len();
                return Ok((bytes, len));
            }
            Escape::LowSlash(high) => {
                if b != b'\\' {
                    return Err(invalid());
                }
                self.escape = Escape::LowU(high);
                return Ok(([0; 4], 0));
            }
            Escape::LowU(high) => {
                if b != b'u' {
                    return Err(invalid());
                }
                self.escape = Escape::Hex {
                    value: 0,
                    digits: 0,
                    high: Some(high),
                };
                return Ok(([0; 4], 0));
            }
        };
        if self.utf8_len == 0 && decoded < 128 {
            return Ok(([decoded, 0, 0, 0], 1));
        }
        if self.utf8_len == 4 {
            return Err(invalid());
        }
        self.utf8[self.utf8_len] = decoded;
        self.utf8_len += 1;
        match std::str::from_utf8(&self.utf8[..self.utf8_len]) {
            Ok(_) => {
                let len = self.utf8_len;
                self.utf8_len = 0;
                Ok((self.utf8, len))
            }
            Err(e) if e.error_len().is_none() => Ok(([0; 4], 0)),
            _ => Err(invalid()),
        }
    }
}

#[derive(Clone)]
enum Kind {
    Object,
    Array,
    String { sequence_key: bool },
    Number(serde_json::Number),
    Bool(bool),
    Null,
}
#[derive(Clone)]
struct Node {
    span: Span,
    kind: Kind,
}
impl Node {
    fn retained(&self) -> usize {
        if matches!(self.kind, Kind::Number(_)) {
            std::mem::size_of::<serde_json::Number>()
        } else {
            0
        }
    }
}
#[derive(Clone, Copy)]
enum Frame {
    ArrayFirst,
    ArrayValue,
    ArrayComma,
    ObjectFirst,
    ObjectKey,
    ObjectColon,
    ObjectValue,
    ObjectComma,
}
enum Token {
    String {
        decoder: Decoder,
        key: bool,
        sequence_at: Option<usize>,
    },
    Number(JsonNumber),
    Literal {
        word: &'static [u8],
        after: usize,
    },
}
struct ValueScan {
    range: Span,
    after: u64,
    start: Option<u64>,
    kind: Option<Kind>,
    frames: Vec<Frame>,
    token: Option<Token>,
    done: bool,
}
impl ValueScan {
    fn new(range: Span, after: u64) -> Self {
        Self {
            range,
            after,
            start: None,
            kind: None,
            frames: Vec::new(),
            token: None,
            done: false,
        }
    }
    fn retained(&self) -> usize {
        match &self.token {
            Some(Token::String { decoder, .. }) => decoder.retained(),
            Some(Token::Number(_)) => std::mem::size_of::<JsonNumber>(),
            _ => 0,
        }
    }
    fn root_done(&mut self) {
        if self.frames.is_empty() {
            self.done = true;
        }
    }
    fn result(&self) -> Result<Node, SidecarError> {
        Ok(Node {
            span: Span {
                start: self.start.ok_or_else(invalid)?,
                end: self.after,
                ..self.range
            },
            kind: self.kind.clone().ok_or_else(invalid)?,
        })
    }
    fn run(&mut self, input: &mut Input<'_>) -> Result<Poll<Node>, SidecarError> {
        loop {
            if self.done {
                return Ok(Poll::Ready(self.result()?));
            }
            let b = match input.at(self.range, self.after)? {
                Read::Byte(b) => b,
                Read::Pending => return Ok(Poll::Pending),
                Read::End => {
                    if let Some(Token::Number(number)) = self.token.take() {
                        let n = number.finish()?;
                        if self.frames.is_empty() {
                            self.kind = Some(Kind::Number(n));
                            self.done = true;
                            input.progress += 1;
                            continue;
                        }
                    }
                    return Err(invalid());
                }
            };
            if let Some(mut token) = self.token.take() {
                let mut done = false;
                match &mut token {
                    Token::String {
                        decoder,
                        key,
                        sequence_at,
                    } => {
                        if b == b'"' && decoder.plain() {
                            done = true;
                            if *key {
                                *self.frames.last_mut().ok_or_else(invalid)? = Frame::ObjectColon;
                            } else if self.frames.is_empty() {
                                self.kind = Some(Kind::String {
                                    sequence_key: *sequence_at == Some(SEQUENCE.len()),
                                });
                                self.root_done();
                            }
                        } else {
                            let (bytes, len) = decoder.accept(b)?;
                            for byte in &bytes[..len] {
                                *sequence_at = sequence_at
                                    .and_then(|i| (SEQUENCE.get(i) == Some(byte)).then_some(i + 1));
                            }
                        }
                    }
                    Token::Number(number) => {
                        if !number.accept(b)? {
                            let n = number.finish()?;
                            if self.frames.is_empty() {
                                self.kind = Some(Kind::Number(n));
                                self.root_done();
                            }
                            input.progress += 1;
                            continue;
                        }
                    }
                    Token::Literal { word, after } => {
                        if word.get(*after) != Some(&b) {
                            return Err(invalid());
                        }
                        *after += 1;
                        done = *after == word.len();
                        if done {
                            self.root_done();
                        }
                    }
                }
                if !done {
                    self.token = Some(token);
                }
                input.advance(&mut self.after);
                continue;
            }
            if whitespace(b) {
                input.advance(&mut self.after);
                continue;
            }
            if !input.unit() {
                return Ok(Poll::Pending);
            }
            match self.frames.last_mut() {
                Some(Frame::ArrayFirst) if b == b']' => {
                    self.frames.pop();
                    self.root_done();
                    input.advance(&mut self.after);
                    continue;
                }
                Some(Frame::ObjectFirst) if b == b'}' => {
                    self.frames.pop();
                    self.root_done();
                    input.advance(&mut self.after);
                    continue;
                }
                Some(frame @ Frame::ArrayComma) => {
                    match b {
                        b',' => *frame = Frame::ArrayValue,
                        b']' => {
                            self.frames.pop();
                            self.root_done();
                        }
                        _ => return Err(invalid()),
                    };
                    input.advance(&mut self.after);
                    continue;
                }
                Some(frame @ Frame::ObjectComma) => {
                    match b {
                        b',' => *frame = Frame::ObjectKey,
                        b'}' => {
                            self.frames.pop();
                            self.root_done();
                        }
                        _ => return Err(invalid()),
                    };
                    input.advance(&mut self.after);
                    continue;
                }
                Some(Frame::ObjectFirst | Frame::ObjectKey) => {
                    if b != b'"' {
                        return Err(invalid());
                    }
                    self.token = Some(Token::String {
                        decoder: Decoder::default(),
                        key: true,
                        sequence_at: Some(0),
                    });
                    input.advance(&mut self.after);
                    continue;
                }
                Some(frame @ Frame::ObjectColon) => {
                    if b != b':' {
                        return Err(invalid());
                    }
                    *frame = Frame::ObjectValue;
                    input.advance(&mut self.after);
                    continue;
                }
                Some(frame @ (Frame::ArrayFirst | Frame::ArrayValue)) => *frame = Frame::ArrayComma,
                Some(frame @ Frame::ObjectValue) => *frame = Frame::ObjectComma,
                None if self.start.is_some() => return Err(invalid()),
                None => self.start = Some(self.after),
            }
            let kind = match b {
                b'[' | b'{' => {
                    if self.frames.len() == DEPTH {
                        return Err(invalid());
                    }
                    self.frames.push(if b == b'[' {
                        Frame::ArrayFirst
                    } else {
                        Frame::ObjectFirst
                    });
                    if b == b'[' {
                        Kind::Array
                    } else {
                        Kind::Object
                    }
                }
                b'"' => {
                    self.token = Some(Token::String {
                        decoder: Decoder::default(),
                        key: false,
                        sequence_at: Some(0),
                    });
                    Kind::String {
                        sequence_key: false,
                    }
                }
                b't' | b'f' | b'n' => {
                    self.token = Some(Token::Literal {
                        word: match b {
                            b't' => b"true",
                            b'f' => b"false",
                            _ => b"null",
                        },
                        after: 1,
                    });
                    match b {
                        b't' => Kind::Bool(true),
                        b'f' => Kind::Bool(false),
                        _ => Kind::Null,
                    }
                }
                b'-' | b'0'..=b'9' => {
                    let mut n = JsonNumber::default();
                    n.accept(b)?;
                    self.token = Some(Token::Number(n));
                    Kind::Null
                }
                _ => return Err(invalid()),
            };
            if self.kind.is_none() {
                self.kind = Some(kind);
            }
            input.advance(&mut self.after);
        }
    }
}

struct DecodedCursor {
    span: Span,
    after: u64,
    decoder: Decoder,
    bytes: [u8; 4],
    next: usize,
    end: usize,
}
impl DecodedCursor {
    fn new(span: Span) -> Self {
        Self {
            span,
            after: span.start,
            decoder: Decoder::default(),
            bytes: [0; 4],
            next: 0,
            end: 0,
        }
    }
    fn retained(&self) -> usize {
        self.decoder.retained() + self.end - self.next
    }
    fn next(&mut self, input: &mut Input<'_>) -> Result<Poll<Option<u8>>, SidecarError> {
        loop {
            if self.next < self.end {
                let b = self.bytes[self.next];
                self.next += 1;
                input.progress += 1;
                return Ok(Poll::Ready(Some(b)));
            }
            match input.at(self.span, self.after)? {
                Read::Pending => return Ok(Poll::Pending),
                Read::End => {
                    if !self.decoder.plain() {
                        return Err(invalid());
                    }
                    return Ok(Poll::Ready(None));
                }
                Read::Byte(b) => {
                    let (bytes, end) = self.decoder.accept(b)?;
                    input.advance(&mut self.after);
                    self.bytes = bytes;
                    self.end = end;
                    self.next = 0;
                }
            }
        }
    }
}
struct DecodedEqual {
    left: DecodedCursor,
    right: DecodedCursor,
    left_byte: Option<Option<u8>>,
}
impl DecodedEqual {
    fn new(left: Span, right: Span) -> Self {
        Self {
            left: DecodedCursor::new(left),
            right: DecodedCursor::new(right),
            left_byte: None,
        }
    }
    fn retained(&self) -> usize {
        self.left.retained() + self.right.retained() + usize::from(self.left_byte.is_some())
    }
    fn run(&mut self, input: &mut Input<'_>) -> Result<Poll<bool>, SidecarError> {
        loop {
            if self.left_byte.is_none() {
                match self.left.next(input)? {
                    Poll::Pending => return Ok(Poll::Pending),
                    Poll::Ready(b) => self.left_byte = Some(b),
                }
            }
            let right = match self.right.next(input)? {
                Poll::Pending => return Ok(Poll::Pending),
                Poll::Ready(b) => b,
            };
            let left = self.left_byte.take().ok_or_else(invalid)?;
            if left != right {
                return Ok(Poll::Ready(false));
            }
            if left.is_none() {
                return Ok(Poll::Ready(true));
            }
        }
    }
}
struct Member {
    key: Node,
    value: Node,
}
impl Member {
    fn retained(&self) -> usize {
        self.value.retained()
    }
    fn sequence(&self) -> bool {
        matches!(self.key.kind, Kind::String { sequence_key: true })
    }
}
struct Members {
    range: Span,
    after: u64,
    phase: u8,
    first: bool,
    scan: Option<ValueScan>,
    key: Option<Node>,
}
impl Members {
    fn new(range: Span) -> Self {
        let range = range.content();
        Self {
            range,
            after: range.start,
            phase: 0,
            first: true,
            scan: None,
            key: None,
        }
    }
    fn retained(&self) -> usize {
        self.scan.as_ref().map_or(0, ValueScan::retained)
    }
    fn next(&mut self, input: &mut Input<'_>) -> Result<Poll<Option<Member>>, SidecarError> {
        loop {
            if let Some(scan) = self.scan.as_mut() {
                let node = match scan.run(input)? {
                    Poll::Pending => return Ok(Poll::Pending),
                    Poll::Ready(n) => n,
                };
                self.after = scan.after;
                self.scan = None;
                input.progress += 1;
                if self.phase == 0 {
                    if !matches!(node.kind, Kind::String { .. }) {
                        return Err(invalid());
                    }
                    self.key = Some(node);
                    self.phase = 1;
                } else {
                    self.phase = 3;
                    self.first = false;
                    return Ok(Poll::Ready(Some(Member {
                        key: self.key.take().ok_or_else(invalid)?,
                        value: node,
                    })));
                }
                continue;
            }
            let b = match input.at(self.range, self.after)? {
                Read::Pending => return Ok(Poll::Pending),
                Read::End => {
                    return if self.phase == 3 || self.first {
                        Ok(Poll::Ready(None))
                    } else {
                        Err(invalid())
                    }
                }
                Read::Byte(b) => b,
            };
            if whitespace(b) {
                input.advance(&mut self.after);
                continue;
            }
            if !input.unit() {
                return Ok(Poll::Pending);
            }
            match self.phase {
                0 | 2 => self.scan = Some(ValueScan::new(self.range, self.after)),
                1 => {
                    if b != b':' {
                        return Err(invalid());
                    }
                    self.phase = 2;
                    input.advance(&mut self.after);
                }
                3 => {
                    if b != b',' {
                        return Err(invalid());
                    }
                    self.phase = 0;
                    input.advance(&mut self.after);
                }
                _ => return Err(invalid()),
            }
        }
    }
}
struct Elements {
    range: Span,
    after: u64,
    comma: bool,
    first: bool,
    scan: Option<ValueScan>,
}
impl Elements {
    fn new(range: Span) -> Self {
        let range = range.content();
        Self {
            range,
            after: range.start,
            comma: false,
            first: true,
            scan: None,
        }
    }
    fn retained(&self) -> usize {
        self.scan.as_ref().map_or(0, ValueScan::retained)
    }
    fn next(&mut self, input: &mut Input<'_>) -> Result<Poll<Option<Node>>, SidecarError> {
        loop {
            if let Some(scan) = self.scan.as_mut() {
                let node = match scan.run(input)? {
                    Poll::Pending => return Ok(Poll::Pending),
                    Poll::Ready(n) => n,
                };
                self.after = scan.after;
                self.scan = None;
                self.comma = true;
                self.first = false;
                input.progress += 1;
                return Ok(Poll::Ready(Some(node)));
            }
            let b = match input.at(self.range, self.after)? {
                Read::Pending => return Ok(Poll::Pending),
                Read::End => {
                    return if self.comma || self.first {
                        Ok(Poll::Ready(None))
                    } else {
                        Err(invalid())
                    }
                }
                Read::Byte(b) => b,
            };
            if whitespace(b) {
                input.advance(&mut self.after);
                continue;
            }
            if !input.unit() {
                return Ok(Poll::Pending);
            }
            if self.comma {
                if b != b',' {
                    return Err(invalid());
                }
                input.advance(&mut self.after);
                self.comma = false;
            } else {
                self.scan = Some(ValueScan::new(self.range, self.after));
            }
        }
    }
}
struct Finder {
    members: Members,
    target: Span,
    current: Option<Member>,
    equal: Option<DecodedEqual>,
    found: Option<Node>,
    presence_only: bool,
}
impl Finder {
    fn new(object: Span, target: Span, presence_only: bool) -> Self {
        Self {
            members: Members::new(object),
            target,
            current: None,
            equal: None,
            found: None,
            presence_only,
        }
    }
    fn retained(&self) -> usize {
        self.members.retained()
            + self.equal.as_ref().map_or(0, DecodedEqual::retained)
            + self.current.as_ref().map_or(0, Member::retained)
            + self.found.as_ref().map_or(0, Node::retained)
    }
    fn run(&mut self, input: &mut Input<'_>) -> Result<Poll<Option<Node>>, SidecarError> {
        loop {
            if self.current.is_none() {
                let member = match self.members.next(input)? {
                    Poll::Pending => return Ok(Poll::Pending),
                    Poll::Ready(Some(m)) => m,
                    Poll::Ready(None) => return Ok(Poll::Ready(self.found.take())),
                };
                self.equal = Some(DecodedEqual::new(self.target, member.key.span.content()));
                self.current = Some(member);
            }
            let matches = match self.equal.as_mut().ok_or_else(invalid)?.run(input)? {
                Poll::Pending => return Ok(Poll::Pending),
                Poll::Ready(m) => m,
            };
            let member = self.current.take().ok_or_else(invalid)?;
            self.equal = None;
            input.progress += 1;
            if matches {
                self.found = Some(member.value);
                if self.presence_only {
                    return Ok(Poll::Ready(self.found.take()));
                }
            }
            // Never stop on an earlier duplicate: serde retains the last value.
        }
    }
}
struct ObjectWork {
    left: Span,
    right: Span,
    members: Members,
    reverse: bool,
    root: bool,
    current: Option<Member>,
    finder: Option<Finder>,
}
impl ObjectWork {
    fn new(left: Span, right: Span, root: bool) -> Self {
        Self {
            left,
            right,
            members: Members::new(left),
            reverse: false,
            root,
            current: None,
            finder: None,
        }
    }
    fn retained(&self) -> usize {
        self.members.retained()
            + self.finder.as_ref().map_or(0, Finder::retained)
            + self.current.as_ref().map_or(0, Member::retained)
    }
    fn run(&mut self, input: &mut Input<'_>) -> Result<Effect, SidecarError> {
        if self.current.is_none() {
            let member = match self.members.next(input)? {
                Poll::Pending => return Ok(Effect::Pending),
                Poll::Ready(Some(m)) => m,
                Poll::Ready(None) => {
                    if self.reverse {
                        return Ok(Effect::Done);
                    }
                    self.reverse = true;
                    self.members = Members::new(self.right);
                    return Ok(Effect::Keep);
                }
            };
            if self.root && member.sequence() {
                return Ok(Effect::Keep);
            }
            self.finder = Some(Finder::new(
                if self.reverse { self.left } else { self.right },
                member.key.span.content(),
                self.reverse,
            ));
            self.current = Some(member);
        }
        let found = match self.finder.as_mut().ok_or_else(invalid)?.run(input)? {
            Poll::Pending => return Ok(Effect::Pending),
            Poll::Ready(n) => n.ok_or_else(invalid)?,
        };
        let current = self.current.take().ok_or_else(invalid)?;
        self.finder = None;
        if self.reverse {
            Ok(Effect::Keep)
        } else {
            Ok(Effect::Push(Work::Pair(current.value, found, false)))
        }
    }
}
enum Work {
    Pair(Node, Node, bool),
    String(DecodedEqual),
    Object(Box<ObjectWork>),
    Array(Box<ArrayWork>),
}
struct ArrayWork {
    left: Elements,
    right: Elements,
    pending: Option<Option<Node>>,
}
enum Effect {
    Pending,
    Keep,
    Done,
    Push(Work),
    Replace(Work),
}
impl Work {
    fn retained(&self) -> usize {
        match self {
            Self::Pair(a, b, _) => a.retained() + b.retained(),
            Self::String(s) => s.retained(),
            Self::Object(s) => s.retained(),
            Self::Array(a) => {
                a.left.retained()
                    + a.right.retained()
                    + a.pending
                        .as_ref()
                        .and_then(Option::as_ref)
                        .map_or(0, Node::retained)
            }
        }
    }
    fn run(&mut self, input: &mut Input<'_>) -> Result<Effect, SidecarError> {
        match self {
            Self::Pair(left, right, root) => match (&left.kind, &right.kind) {
                (Kind::Object, Kind::Object) => Ok(Effect::Replace(Self::Object(Box::new(
                    ObjectWork::new(left.span, right.span, *root),
                )))),
                (Kind::Array, Kind::Array) => {
                    Ok(Effect::Replace(Self::Array(Box::new(ArrayWork {
                        left: Elements::new(left.span),
                        right: Elements::new(right.span),
                        pending: None,
                    }))))
                }
                (Kind::String { .. }, Kind::String { .. }) => Ok(Effect::Replace(Self::String(
                    DecodedEqual::new(left.span.content(), right.span.content()),
                ))),
                (Kind::Number(a), Kind::Number(b)) if a == b => Ok(Effect::Done),
                (Kind::Bool(a), Kind::Bool(b)) if a == b => Ok(Effect::Done),
                (Kind::Null, Kind::Null) => Ok(Effect::Done),
                _ => Err(invalid()),
            },
            Self::String(equal) => match equal.run(input)? {
                Poll::Pending => Ok(Effect::Pending),
                Poll::Ready(true) => Ok(Effect::Done),
                Poll::Ready(false) => Err(invalid()),
            },
            Self::Object(object) => object.run(input),
            Self::Array(array) => {
                let ArrayWork {
                    left,
                    right,
                    pending,
                } = array.as_mut();
                if pending.is_none() {
                    match left.next(input)? {
                        Poll::Pending => return Ok(Effect::Pending),
                        Poll::Ready(n) => *pending = Some(n),
                    }
                }
                let right = match right.next(input)? {
                    Poll::Pending => return Ok(Effect::Pending),
                    Poll::Ready(n) => n,
                };
                match (pending.take().ok_or_else(invalid)?, right) {
                    (None, None) => Ok(Effect::Done),
                    (Some(a), Some(b)) => Ok(Effect::Push(Self::Pair(a, b, false))),
                    _ => Err(invalid()),
                }
            }
        }
    }
}
struct Meaning {
    source: Option<ValueScan>,
    projection: Option<Node>,
    work: Vec<Work>,
}
impl Meaning {
    fn new(source: Span, projection: Node) -> Self {
        Self {
            source: Some(ValueScan::new(source, source.start)),
            projection: Some(projection),
            work: Vec::new(),
        }
    }
    fn retained(&self) -> usize {
        self.source.as_ref().map_or(0, ValueScan::retained)
            + self.work.iter().map(Work::retained).sum::<usize>()
    }
    fn run(&mut self, input: &mut Input<'_>) -> Result<Poll<()>, SidecarError> {
        if let Some(scan) = self.source.as_mut() {
            let source = match scan.run(input)? {
                Poll::Pending => return Ok(Poll::Pending),
                Poll::Ready(n) => n,
            };
            if !matches!(source.kind, Kind::Object) {
                return Err(invalid());
            }
            self.work.push(Work::Pair(
                source,
                self.projection.take().ok_or_else(invalid)?,
                true,
            ));
            self.source = None;
            input.progress += 1;
        }
        while let Some(work) = self.work.last_mut() {
            if !input.unit() {
                return Ok(Poll::Pending);
            }
            let effect = work.run(input)?;
            match effect {
                Effect::Pending => return Ok(Poll::Pending),
                Effect::Keep => {}
                Effect::Done => {
                    self.work.pop();
                }
                Effect::Replace(next) => *work = next,
                Effect::Push(next) => {
                    if self.work.len() > DEPTH {
                        return Err(invalid());
                    }
                    self.work.push(next);
                }
            }
            input.progress += 1;
        }
        Ok(Poll::Ready(()))
    }
}

struct Syntax {
    scan: ValueScan,
    root: Option<Node>,
}
impl Syntax {
    fn new(range: Span) -> Self {
        Self {
            scan: ValueScan::new(range, range.start),
            root: None,
        }
    }
    fn run(&mut self, input: &mut Input<'_>) -> Result<Poll<Node>, SidecarError> {
        if self.root.is_none() {
            let root = match self.scan.run(input)? {
                Poll::Pending => return Ok(Poll::Pending),
                Poll::Ready(n) => n,
            };
            if !matches!(root.kind, Kind::Object) {
                return Err(invalid());
            }
            self.root = Some(root);
            input.progress += 1;
        }
        loop {
            match input.at(self.scan.range, self.scan.after)? {
                Read::Pending => return Ok(Poll::Pending),
                Read::End => return Ok(Poll::Ready(self.root.take().ok_or_else(invalid)?)),
                Read::Byte(b) => {
                    if !whitespace(b) {
                        return Err(invalid());
                    }
                    input.advance(&mut self.scan.after);
                }
            }
        }
    }
}
enum Phase {
    Canonical,
    Syntax(Syntax),
    Meaning(Meaning),
    Mirror,
}
impl Phase {
    fn retained(&self) -> usize {
        match self {
            Self::Syntax(s) => s.scan.retained(),
            Self::Meaning(m) => m.retained(),
            _ => 0,
        }
    }
    #[cfg(test)]
    fn name(&self) -> &'static str {
        match self {
            Self::Canonical => "canonical",
            Self::Syntax(_) => "syntax",
            Self::Meaning(_) => "meaning",
            Self::Mirror => "mirror",
        }
    }
}
struct Progress {
    source: CanonicalProgress,
    projection: ProjectionBinding,
    phase: Phase,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ProjectionCompatibilityStep {
    Pending(ByteUsage),
    Cancelled,
    ProjectionCompatible { usage: ByteUsage },
}
pub(crate) struct ProjectionCompatibilitySession<'connection> {
    connection: &'connection mut Connection,
    epoch: i64,
    progress: Option<Progress>,
    #[cfg(test)]
    last_usage: ByteUsage,
    #[cfg(test)]
    limit: usize,
}
impl<'connection> ProjectionCompatibilitySession<'connection> {
    pub(crate) fn start(
        connection: &'connection mut Connection,
        column: RawColumn,
        expected: SourceIdentity,
    ) -> Result<Self, SidecarError> {
        if !connection.is_autocommit()
            || matches!(column, RawColumn::FrozenRecord)
            || expected.source_rowid <= 0
        {
            return Err(invalid());
        }
        // Projection retains its own closed primary-key binding. Preflight that
        // duplicate ID before allocating it, then charge both metadata reads.
        let retained = expected
            .bytes()?
            .checked_add(expected.aggregate_id.len())
            .ok_or_else(invalid)?;
        if retained > INPUT_BYTES {
            return Err(invalid());
        }
        let mut usage = ByteUsage {
            retained_bytes: retained,
            ..ByteUsage::default()
        };
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| invalid())?;
        let epoch = data_version(&tx)?;
        preflight_schema_and_control_rows(&tx)?;
        verify_schema_v8(&tx)?;
        let source = BoundColumn::bind(&tx, column, expected, &mut usage)?;
        let projection = source.bind_projection(&tx, &mut usage)?;
        if usage.encoded_bytes() >= INPUT_BYTES {
            return Err(invalid());
        }
        tx.commit().map_err(|_| invalid())?;
        if !connection.is_autocommit() {
            return Err(invalid());
        }
        Ok(Self {
            connection,
            epoch,
            progress: Some(Progress {
                source: CanonicalProgress::new(source),
                projection,
                phase: Phase::Canonical,
            }),
            #[cfg(test)]
            last_usage: usage,
            #[cfg(test)]
            limit: CHUNK,
        })
    }
    pub(crate) fn step(
        &mut self,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<ProjectionCompatibilityStep, SidecarError> {
        let mut progress = self.progress.take().ok_or_else(invalid)?;
        #[cfg(test)]
        {
            self.last_usage = ByteUsage::default();
        }
        if !self.connection.is_autocommit() {
            return Err(invalid());
        }
        let retained = progress
            .source
            .retained_bytes()?
            .checked_add(progress.projection.retained_bytes())
            .and_then(|n| n.checked_add(progress.phase.retained()))
            .ok_or_else(invalid)?;
        if retained > INPUT_BYTES {
            return Err(invalid());
        }
        let mut usage = ByteUsage {
            retained_bytes: retained,
            ..ByteUsage::default()
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| invalid())?;
        #[cfg(test)]
        let limit = self.limit;
        #[cfg(not(test))]
        let limit = CHUNK;
        let checked = (|| {
            if data_version(&tx)? != self.epoch {
                return Err(invalid());
            }
            if cancelled() {
                return Ok(Disposition::Cancelled);
            }
            progress.source.bound().recheck(&tx, &mut usage)?;
            progress.projection.recheck(&tx, &mut usage)?;
            if matches!(progress.phase, Phase::Canonical) {
                let mut canonical = CanonicalUsage {
                    input: std::mem::take(&mut usage),
                    ..CanonicalUsage::default()
                };
                let result = progress.source.scan(&tx, &mut canonical, limit, cancelled);
                usage = canonical.input;
                match result? {
                    Disposition::Cancelled => return Ok(Disposition::Cancelled),
                    Disposition::Pending => {}
                    Disposition::Complete => {
                        progress.phase = Phase::Syntax(Syntax::new(Span {
                            side: Side::Projection,
                            start: 0,
                            end: progress.projection.byte_length,
                        }))
                    }
                }
            } else if matches!(progress.phase, Phase::Mirror) {
                progress.projection.mirror(&tx, &mut usage)?;
                if cancelled() {
                    return Ok(Disposition::Cancelled);
                }
                return Ok(Disposition::Complete);
            } else {
                let mut input = Input {
                    connection: &tx,
                    source: progress.source.bound(),
                    projection: &progress.projection,
                    usage: &mut usage,
                    cache: [None, None],
                    progress: 0,
                    limit,
                    cancelled,
                    cancelled_seen: false,
                };
                let next = match &mut progress.phase {
                    Phase::Syntax(syntax) => match syntax.run(&mut input)? {
                        Poll::Pending => None,
                        Poll::Ready(root) => Some(Phase::Meaning(Meaning::new(
                            Span {
                                side: Side::Source,
                                start: 0,
                                end: progress.source.bound().byte_length,
                            },
                            root,
                        ))),
                    },
                    Phase::Meaning(meaning) => match meaning.run(&mut input)? {
                        Poll::Pending => None,
                        Poll::Ready(()) => Some(Phase::Mirror),
                    },
                    _ => return Err(invalid()),
                };
                if input.cancelled_seen {
                    return Ok(Disposition::Cancelled);
                }
                if let Some(next) = next {
                    progress.phase = next;
                } else if input.progress == 0 {
                    return Err(invalid());
                }
            }
            if cancelled() {
                return Ok(Disposition::Cancelled);
            }
            Ok(Disposition::Pending)
        })();
        #[cfg(test)]
        {
            self.last_usage = ByteUsage {
                retained_bytes: usage.retained_bytes,
                metadata_bytes: usage.metadata_bytes,
                fragment_bytes: usage.fragment_bytes,
                work_units: usage.work_units,
            };
        }
        match checked {
            Err(error) => {
                tx.rollback().map_err(|_| invalid())?;
                Err(error)
            }
            Ok(Disposition::Cancelled) => {
                tx.rollback().map_err(|_| invalid())?;
                Ok(ProjectionCompatibilityStep::Cancelled)
            }
            Ok(result) => {
                tx.commit().map_err(|_| invalid())?;
                if !self.connection.is_autocommit() {
                    return Err(invalid());
                }
                match result {
                    Disposition::Complete => {
                        Ok(ProjectionCompatibilityStep::ProjectionCompatible { usage })
                    }
                    Disposition::Pending => {
                        self.progress = Some(progress);
                        Ok(ProjectionCompatibilityStep::Pending(usage))
                    }
                    Disposition::Cancelled => unreachable!(),
                }
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn phase_for_test(&self) -> Option<&'static str> {
        self.progress.as_ref().map(|p| p.phase.name())
    }
    #[cfg(test)]
    pub(crate) fn usage_for_test(&self) -> &ByteUsage {
        &self.last_usage
    }
    #[cfg(test)]
    pub(crate) fn limit_for_test(&mut self, limit: usize) {
        assert!((1..=CHUNK).contains(&limit));
        self.limit = limit;
    }
    #[cfg(test)]
    pub(crate) fn autocommit_for_test(&self) -> bool {
        self.connection.is_autocommit()
    }
    #[cfg(test)]
    pub(crate) fn projection_queries_for_test(&self) -> Vec<String> {
        self.progress
            .as_ref()
            .unwrap()
            .projection
            .queries_for_test()
    }
    #[cfg(test)]
    pub(crate) fn nested_meaning_for_test(&self) -> bool {
        matches!(self.progress.as_ref().map(|p| &p.phase), Some(Phase::Meaning(meaning)) if meaning.source.is_none() && !meaning.work.is_empty())
    }
}

#[cfg(test)]
pub(crate) fn number_for_test(
    raw: &[u8],
    chunk: usize,
) -> Result<serde_json::Number, SidecarError> {
    let mut number = JsonNumber::default();
    for fragment in raw.chunks(chunk) {
        for b in fragment {
            if !number.accept(*b)? {
                return Err(invalid());
            }
        }
    }
    number.finish()
}
