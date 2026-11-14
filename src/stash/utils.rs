use crate::proto::galadh::KeyValue;
use crate::trie::KVPair;
use std::io;
use std::path::Path;
use std::string::FromUtf8Error;

pub(crate) fn read_key_val(key: Vec<u8>, val: Vec<u8>) -> Result<(String, String), FromUtf8Error> {
    let key = String::from_utf8(key)?;
    let val = String::from_utf8(val)?;
    Ok((key, val))
}
pub(crate) fn create_kv(key: &str, val: &str) -> KeyValue {
    KeyValue {
        key: key.into(),
        value: val.into(),
        lease: 0,
    }
}

/// Snapshot format: repeated (u64 key length, key bytes, u64 value length,
/// value bytes) in host byte order.
pub(crate) fn save_snapshot(path: &Path, kvs: &[KVPair]) -> std::io::Result<()> {
    let mut buf = Vec::new();
    for kv in kvs {
        buf.extend_from_slice(&(kv.0.len() as u64).to_le_bytes());
        buf.extend_from_slice(kv.0.as_bytes());
        buf.extend_from_slice(&(kv.1.len() as u64).to_le_bytes());
        buf.extend_from_slice(kv.1.as_bytes());
    }
    // write to a temp file first so a crash cannot leave a truncated snapshot
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &buf)?;
    std::fs::rename(&tmp, path)
}

pub(crate) fn load_snapshot(path: &Path) -> std::io::Result<Vec<KVPair>> {
    let buf = std::fs::read(path)?;
    let mut kvs = Vec::new();
    let mut pos = 0usize;
    while pos < buf.len() {
        if pos + 8 > buf.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated key length"));
        }
        let key_len = u64::from_le_bytes(buf[pos..pos + 8].try_into().unwrap()) as usize;
        pos += 8;
        if pos + key_len > buf.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated key"));
        }
        let key = String::from_utf8_lossy(&buf[pos..pos + key_len]).into_owned();
        pos += key_len;
        if pos + 8 > buf.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated value length"));
        }
        let val_len = u64::from_le_bytes(buf[pos..pos + 8].try_into().unwrap()) as usize;
        pos += 8;
        if pos + val_len > buf.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated value"));
        }
        let val = String::from_utf8_lossy(&buf[pos..pos + val_len]).into_owned();
        pos += val_len;
        kvs.push(KVPair(key, val));
    }
    Ok(kvs)
}

/// etcd-style range end for a prefix scan: increment the last byte,
/// carrying over 0xff bytes. A key of all 0xff has no successor,
/// so "\\0" (all keys >= key) is used instead.
#[allow(dead_code)]
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
