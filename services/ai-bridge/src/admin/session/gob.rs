use std::collections::HashMap;

const MAX_MAP_ENTRIES: usize = 64;
const MAX_SCALAR_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum GobValue {
    Int(i64),
    String(String),
    Bool(bool),
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GobError {
    UnexpectedEof,
    InvalidFormat,
    LimitExceeded,
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_byte(&mut self) -> Result<u8, GobError> {
        let byte = *self.data.get(self.pos).ok_or(GobError::UnexpectedEof)?;
        self.pos += 1;
        Ok(byte)
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], GobError> {
        let end = self.pos.checked_add(len).ok_or(GobError::LimitExceeded)?;
        let bytes = self
            .data
            .get(self.pos..end)
            .ok_or(GobError::UnexpectedEof)?;
        self.pos = end;
        Ok(bytes)
    }

    fn read_uint(&mut self) -> Result<u64, GobError> {
        let first = self.read_byte()?;
        if first <= 0x7f {
            return Ok(u64::from(first));
        }

        let byte_count = usize::from(first.wrapping_neg());
        if byte_count > 8 {
            return Err(GobError::InvalidFormat);
        }

        let mut value = 0_u64;
        for _ in 0..byte_count {
            value = (value << 8) | u64::from(self.read_byte()?);
        }
        Ok(value)
    }

    fn read_int(&mut self) -> Result<i64, GobError> {
        let value = self.read_uint()?;
        if value & 1 == 0 {
            Ok((value >> 1) as i64)
        } else {
            Ok(!((value >> 1) as i64))
        }
    }

    fn read_string(&mut self) -> Result<String, GobError> {
        let len = usize::try_from(self.read_uint()?).map_err(|_| GobError::LimitExceeded)?;
        if len > MAX_SCALAR_BYTES {
            return Err(GobError::LimitExceeded);
        }
        String::from_utf8(self.read_bytes(len)?.to_vec()).map_err(|_| GobError::InvalidFormat)
    }
}

pub(super) fn decode_session_map(data: &[u8]) -> Result<HashMap<String, GobValue>, GobError> {
    let mut reader = Reader::new(data);

    loop {
        let message_len =
            usize::try_from(reader.read_uint()?).map_err(|_| GobError::LimitExceeded)?;
        if message_len == 0 || message_len > reader.remaining() {
            return Err(GobError::UnexpectedEof);
        }

        let message_start = reader.pos;
        let type_id = reader.read_int()?;
        if type_id >= 0 {
            return decode_map_data(&mut reader);
        }

        reader.pos = message_start
            .checked_add(message_len)
            .ok_or(GobError::LimitExceeded)?;
    }
}

fn decode_map_data(reader: &mut Reader<'_>) -> Result<HashMap<String, GobValue>, GobError> {
    if reader.read_uint()? != 0 {
        return Err(GobError::InvalidFormat);
    }

    let count = usize::try_from(reader.read_uint()?).map_err(|_| GobError::LimitExceeded)?;
    if count > MAX_MAP_ENTRIES {
        return Err(GobError::LimitExceeded);
    }

    let mut map = HashMap::with_capacity(count);
    for _ in 0..count {
        let GobValue::String(key) = decode_interface_value(reader)? else {
            return Err(GobError::InvalidFormat);
        };
        map.insert(key, decode_interface_value(reader)?);
    }
    Ok(map)
}

fn decode_interface_value(reader: &mut Reader<'_>) -> Result<GobValue, GobError> {
    let type_name = reader.read_string()?;
    let _type_id = reader.read_uint()?;
    let value_len = usize::try_from(reader.read_uint()?).map_err(|_| GobError::LimitExceeded)?;
    if value_len > MAX_SCALAR_BYTES {
        return Err(GobError::LimitExceeded);
    }
    let value_bytes = reader.read_bytes(value_len)?;

    if !is_supported_scalar_type(&type_name) {
        return Ok(GobValue::Unsupported);
    }

    let mut value_reader = Reader::new(value_bytes);
    if value_reader.read_byte()? != 0 {
        return Err(GobError::InvalidFormat);
    }

    match type_name.as_str() {
        "string" => Ok(GobValue::String(value_reader.read_string()?)),
        "int" | "int8" | "int16" | "int32" | "int64" => Ok(GobValue::Int(value_reader.read_int()?)),
        "uint" | "uint8" | "uint16" | "uint32" | "uint64" | "uintptr" => {
            let value =
                i64::try_from(value_reader.read_uint()?).map_err(|_| GobError::LimitExceeded)?;
            Ok(GobValue::Int(value))
        }
        "bool" => match value_reader.read_uint()? {
            0 => Ok(GobValue::Bool(false)),
            1 => Ok(GobValue::Bool(true)),
            _ => Err(GobError::InvalidFormat),
        },
        _ => Ok(GobValue::Unsupported),
    }
}

fn is_supported_scalar_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "string"
            | "int"
            | "int8"
            | "int16"
            | "int32"
            | "int64"
            | "uint"
            | "uint8"
            | "uint16"
            | "uint32"
            | "uint64"
            | "uintptr"
            | "bool"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_new_api_session_map() {
        let data = hex::decode("0d7f040102ff8000011001100000ff90ff80000506737472696e670c0a0008757365726e616d6506737472696e670c07000561646d696e06737472696e670c060004726f6c6503696e74040400fe07d006737472696e670c08000673746174757303696e740402000206737472696e670c07000567726f757006737472696e670c09000764656661756c7406737472696e670c040002696403696e7404020002").unwrap();
        let map = decode_session_map(&data).unwrap();

        assert_eq!(map.get("id"), Some(&GobValue::Int(1)));
        assert_eq!(map.get("username"), Some(&GobValue::String("admin".into())));
        assert_eq!(map.get("role"), Some(&GobValue::Int(1000)));
        assert_eq!(map.get("status"), Some(&GobValue::Int(1)));
        assert_eq!(map.get("group"), Some(&GobValue::String("default".into())));
    }
}
