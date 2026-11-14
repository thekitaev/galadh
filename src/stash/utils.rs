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
pub(crate) fn create_kv(key: &str, val: &str) -> KeyValue {
    KeyValue {
        key: key.into(),
        value: val.into(),
        lease: 0,
    }
}

/// etcd-style range end for a prefix scan: increment the last byte,
/// carrying over 0xff bytes. A key of all 0xff has no successor,
/// so "\\0" (all keys >= key) is used instead.
pub(crate) fn prefix_end(key: &[u8]) -> Vec<u8> {
    let mut end = key.to_vec();
    for i in (0..end.len()).rev() {
        if end[i] < 0xff {
            end[i] += 1;
            return end;
        }
    }
    vec![0]
}
