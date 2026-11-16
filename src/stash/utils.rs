use crate::proto::galadh::KeyValue;
use crate::trie::KVPair;
use std::io;
use std::path::Path;

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

    // Reads a u64 length prefix at `pos` and checks it against the bytes
    // actually left in the buffer, so a corrupt length prefix is rejected
    // instead of overflowing usize or panicking on a wrapped slice range.
    fn next_len(buf: &[u8], pos: &mut usize, what: &str) -> std::io::Result<usize> {
        if *pos + 8 > buf.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("truncated {what} length"),
            ));
        }
        let n = u64::from_le_bytes(buf[*pos..*pos + 8].try_into().unwrap());
        *pos += 8;
        if n > (buf.len() - *pos) as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("truncated {what}"),
            ));
        }
        Ok(n as usize)
    }

    while pos < buf.len() {
        let key_len = next_len(&buf, &mut pos, "key")?;
        let key = String::from_utf8_lossy(&buf[pos..pos + key_len]).into_owned();
        pos += key_len;
        let val_len = next_len(&buf, &mut pos, "value")?;
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
