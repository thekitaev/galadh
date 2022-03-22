use crate::proto::galadh::KeyValue;
use std::string::FromUtf8Error;

pub(crate) fn read_key_val(key: Vec<u8>, val: Vec<u8>) -> Result<(String, String), FromUtf8Error> {
    let key_result = String::from_utf8(key);
    let new_val_result = String::from_utf8(val);

    let key = match key_result {
        Err(e) => return Err(e),
        Ok(k) => k,
    };

    let val = match new_val_result {
        Err(e) => return Err(e),
        Ok(v) => v,
    };

    Ok((key, val))
}
pub(crate) fn create_kv(key: &str, val: &str, lease: i64) -> KeyValue {
    KeyValue {
        key: key.into(),
        value: val.into(),
        lease: lease,
    }
}
