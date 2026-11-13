//! Incremental JSON array/keyed-object framing. Only one row is retained;
//! serde validates each completed row. No catalog-sized JSON value/tree exists.
use crate::ProviderError;
use iptv_core::import_limits::{ImportLimits, LimitExceeded, LimitKind};

#[derive(Clone, Copy)]
enum State {
    Start,
    ArrayValue(bool),
    ArrayComma,
    Key,
    Colon,
    MapValue,
    MapComma,
    End,
}
struct Token {
    bytes: Vec<u8>,
    depth: usize,
    quoted: bool,
    escaped: bool,
    container: bool,
}
impl Token {
    fn new(byte: u8) -> Self {
        Self {
            bytes: vec![byte],
            depth: usize::from(matches!(byte, b'{' | b'[')),
            quoted: byte == b'"',
            escaped: false,
            container: matches!(byte, b'{' | b'['),
        }
    }
    /// Returns (complete, consumed).
    fn push(&mut self, byte: u8) -> (bool, bool) {
        if !self.quoted
            && self.depth == 0
            && !self.container
            && (byte.is_ascii_whitespace() || matches!(byte, b',' | b']' | b'}'))
        {
            return (true, false);
        }
        self.bytes.push(byte);
        if self.quoted {
            if self.escaped {
                self.escaped = false;
            } else if byte == b'\\' {
                self.escaped = true;
            } else if byte == b'"' {
                self.quoted = false;
                return (self.depth == 0, true);
            }
        } else {
            match byte {
                b'"' => self.quoted = true,
                b'{' | b'[' => self.depth += 1,
                b'}' | b']' => {
                    self.depth = self.depth.saturating_sub(1);
                    return (self.depth == 0, true);
                }
                _ => {}
            }
        }
        (false, true)
    }
}

pub(crate) struct JsonList {
    state: State,
    token: Option<Token>,
    limits: ImportLimits,
    bytes: u64,
    count: u64,
    prefix: Vec<u8>,
    started: bool,
    failed: Option<ProviderError>,
}
fn invalid() -> ProviderError {
    ProviderError::InvalidBody("expected a complete JSON list".into())
}
impl JsonList {
    pub fn new(limits: ImportLimits) -> Self {
        Self {
            state: State::Start,
            token: None,
            limits,
            bytes: 0,
            count: 0,
            prefix: Vec::new(),
            started: false,
            failed: None,
        }
    }
    pub fn feed(
        &mut self,
        bytes: &[u8],
        emit: &mut impl FnMut(&[u8], usize),
    ) -> Result<(), ProviderError> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        self.bytes = self.bytes.saturating_add(bytes.len() as u64);
        let result: Result<(), ProviderError> = (|| {
            LimitExceeded::check(LimitKind::InputBytes, self.bytes, self.limits.input_bytes)
                .map_err(ProviderError::Limit)?;
            let mut input = bytes;
            if !self.started {
                let n = (3 - self.prefix.len()).min(input.len());
                self.prefix.extend_from_slice(&input[..n]);
                input = &input[n..];
                if self.prefix.len() < 3 {
                    return Ok(());
                }
                self.started = true;
                let prefix = std::mem::take(&mut self.prefix);
                for &byte in prefix.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&prefix) {
                    self.byte(byte, emit)?;
                }
            }
            for &byte in input {
                self.byte(byte, emit)?;
            }
            Ok(())
        })();
        if let Err(e) = &result {
            self.failed = Some(e.clone());
        }
        result
    }
    fn complete(
        &mut self,
        bytes: &[u8],
        emit: &mut impl FnMut(&[u8], usize),
    ) -> Result<(), ProviderError> {
        match self.state {
            State::Key => {
                let key: String = serde_json::from_slice(bytes).map_err(|_| invalid())?;
                if key == "user_info" {
                    return Err(ProviderError::AccountRejected);
                }
                self.state = State::Colon;
            }
            State::ArrayValue(_) | State::MapValue => {
                if matches!(self.state, State::MapValue) && !bytes.starts_with(b"{") {
                    return Err(invalid());
                }
                self.count = self.count.saturating_add(1);
                LimitExceeded::check(LimitKind::Items, self.count, self.limits.items)
                    .map_err(ProviderError::Limit)?;
                // Syntax must be valid even when the row isn't an object. IgnoredAny
                // consumes nested unknowns without constructing a Value tree.
                serde_json::from_slice::<serde::de::IgnoredAny>(bytes).map_err(|_| invalid())?;
                emit(bytes, (self.count - 1) as usize);
                self.state = if matches!(self.state, State::MapValue) {
                    State::MapComma
                } else {
                    State::ArrayComma
                };
            }
            _ => return Err(invalid()),
        }
        Ok(())
    }
    fn byte(&mut self, byte: u8, emit: &mut impl FnMut(&[u8], usize)) -> Result<(), ProviderError> {
        if let Some(token) = &mut self.token {
            let (complete, consumed) = token.push(byte);
            LimitExceeded::check(
                LimitKind::RecordBytes,
                token.bytes.len() as u64,
                self.limits.record_bytes as u64,
            )
            .map_err(ProviderError::Limit)?;
            if complete {
                let token = self.token.take().unwrap();
                self.complete(&token.bytes, emit)?;
            } else {
                return Ok(());
            }
            if consumed {
                return Ok(());
            }
        }
        if byte.is_ascii_whitespace() {
            return Ok(());
        }
        match (self.state, byte) {
            (State::Start, b'[') => self.state = State::ArrayValue(true),
            (State::Start, b'{') => self.state = State::Key,
            (State::ArrayValue(true), b']') | (State::ArrayComma, b']') => self.state = State::End,
            (State::MapComma, b'}') => self.state = State::End,
            (State::ArrayComma, b',') => self.state = State::ArrayValue(false),
            (State::MapComma, b',') => self.state = State::Key,
            (State::Colon, b':') => self.state = State::MapValue,
            (State::Key, b'"') => self.token = Some(Token::new(byte)),
            (State::ArrayValue(_) | State::MapValue, _)
                if !matches!(byte, b',' | b']' | b'}' | b':') =>
            {
                self.token = Some(Token::new(byte))
            }
            _ => return Err(invalid()),
        }
        Ok(())
    }
    pub fn finish(&mut self, emit: &mut impl FnMut(&[u8], usize)) -> Result<(), ProviderError> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        if !self.started {
            let prefix = std::mem::take(&mut self.prefix);
            for byte in prefix {
                self.byte(byte, emit)?;
            }
        }
        if matches!(self.state, State::End) && self.token.is_none() {
            Ok(())
        } else {
            Err(invalid())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ambiguous_empty_objects_and_trailing_commas_are_refused() {
        for (bytes, valid) in [
            (b"{}".as_slice(), false),
            (b"[]", true),
            (br#"{"1":{},}"#, false),
            (b"[{},]", false),
        ] {
            let mut p = JsonList::new(Default::default());
            let result = p
                .feed(bytes, &mut |_, _| {})
                .and_then(|_| p.finish(&mut |_, _| {}));
            assert_eq!(result.is_ok(), valid);
        }
    }
}
