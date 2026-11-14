use std::collections::{hash_map::Entry, HashMap};

#[derive(Debug)]
struct TrieNode {
    children: HashMap<u8, TrieNode>,
    kv: Option<KVPair>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KVPair(pub String, pub String);

impl TrieNode {
    fn new() -> Self {
        Self {
            children: HashMap::new(),
            kv: None,
        }
    }
    fn get(&self, key_end: &[u8]) -> Option<KVPair> {
        if key_end.is_empty() {
            return self.kv.clone();
        }
        if !self.children.contains_key(&key_end[0]) {
            None
        } else {
            self.children.get(&key_end[0]).unwrap().get(&key_end[1..])
        }
    }
    fn put(&mut self, full_key: &str, key_end: &[u8], value: &str) -> Option<KVPair> {
        if key_end.is_empty() {
            let old_kv = self.kv.clone();
            self.kv = Some(KVPair(full_key.to_string(), value.to_string()));
            return old_kv;
        }
        if let Entry::Vacant(e) = self.children.entry(key_end[0]) {
            let mut new_node = TrieNode::new();
            let result = new_node.put(full_key, &key_end[1..], value);
            e.insert(new_node);
            result
        } else {
            self.children
                .get_mut(&key_end[0])
                .unwrap()
                .put(full_key, &key_end[1..], value)
        }
    }
    /// return value indicates that the node itself can be deleted
    fn delete(&mut self, key_end: &[u8]) -> bool {
        if key_end.is_empty() {
            self.kv = None;
            return self.children.is_empty();
        }
        if self.children.contains_key(&key_end[0]) {
            let remove_child = self
                .children
                .get_mut(&key_end[0])
                .unwrap()
                .delete(&key_end[1..]);
            if remove_child {
                self.children.remove(&key_end[0]);
            }
        }
        self.children.is_empty()
    }
    fn traverse(&self, out: &mut Vec<KVPair>) {
        if let Some(kv) = self.kv.clone() {
            out.push(kv);
        }
        for node in self.children.values() {
            node.traverse(out);
        }
    }
}

pub struct TrieMap {
    root: TrieNode,
}

impl Default for TrieMap {
    fn default() -> Self {
        Self::new()
    }
}

impl TrieMap {
    pub fn new() -> Self {
        Self {
            root: TrieNode::new(),
        }
    }
    pub fn get(&self, key: &str) -> Option<KVPair> {
        self.root.get(key.as_bytes())
    }
    pub fn put(&mut self, key: &str, value: &str) -> Option<KVPair> {
        self.root.put(key, key.as_bytes(), value)
    }
    pub fn delete(&mut self, key: &str) {
        self.root.delete(key.as_bytes());
    }
    pub fn traverse(&self) -> Vec<KVPair> {
        let mut out = vec![];
        self.root.traverse(&mut out);
        out.sort_by(|a, b|a.0.cmp(&b.0));
        out
    }

    /// Collect all pairs with `from <= key < to` (or `from <= key` when `to`
    /// is None). Unlike `traverse`, subtrees that cannot contain a key
    /// inside the range are pruned during descent.
    pub fn scan(&self, from: &str, to: Option<&str>) -> Vec<KVPair> {
        let mut out = vec![];
        self.root.scan(from.as_bytes(), to, &mut Vec::new(), &mut out);
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
}

impl TrieNode {
    /// Depth-first scan with pruning. `path` accumulates the bytes leading
    /// to `self`.
    fn scan(
        &self,
        from: &[u8],
        to: Option<&str>,
        path: &mut Vec<u8>,
        out: &mut Vec<KVPair>,
    ) {
        if let Some(key) = std::str::from_utf8(path).ok().filter(|_| {
            !path.is_empty()
                && path.as_slice() >= from
                && to.is_none_or(|t| path.as_slice() < t.as_bytes())
        }) {
            if let Some(kv) = &self.kv {
                out.push(KVPair(key.to_string(), kv.1.clone()));
            }
        }

        for (byte, child) in &self.children {
            path.push(*byte);

            // every key in this subtree starts with `path` and is therefore
            // >= path; prune when even the smallest possible key is past the
            // exclusive upper bound. Compare raw bytes: `path` may end in the
            // middle of a multi-byte UTF-8 character, and a lossy str
            // conversion would reorder it (U+FFFD sorts above most bytes).
            let past_upper = to.is_some_and(|t| path.as_slice() >= t.as_bytes());
            // prune when no key in the subtree can reach `from`: path < from
            // and the difference is not inside the path itself
            let before_lower = path.as_slice() < from
                && !from.starts_with(&path[..]);

            if past_upper || before_lower {
                path.pop();
                continue;
            }
            child.scan(from, to, path, out);
            path.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::vec;

    use crate::trie::KVPair;

    use super::TrieMap;

    #[test]
    fn test_trie_put() {
        let mut trie = TrieMap::new();
        trie.put("abc", "def");

        let a_node = trie.root.children.get(&b'a').unwrap();
        let b_node = a_node.children.get(&b'b').unwrap();
        let c_node = b_node.children.get(&b'c').unwrap();

        assert!(a_node.kv.is_none());
        assert!(b_node.kv.is_none());
        assert!(c_node.kv.is_some());
        assert_eq!(trie.get("abc").unwrap().1, "def".to_string());
    }

    #[test]
    fn test_trie_get() {
        let mut trie = TrieMap::new();
        trie.put("key", "value");

        assert_eq!(trie.get("key").unwrap().1, "value".to_string());
    }

    #[test]
    fn test_trie_delete() {
        let mut trie = TrieMap::new();
        trie.put("key", "value");
        trie.put("keys", "values");

        assert!(trie.get("key").is_some());

        trie.delete("key");

        assert!(trie.get("key").is_none());
        assert!(trie.get("keys").is_some());
    }

    #[test]
    fn test_trie_traverse() {
        let mut trie = TrieMap::new();
        trie.put("key", "value");

        let out = trie.traverse();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "key".to_string());

        let all_keys = vec!["key_0", "key_3", "key_2", "key_4", "key_30"];
        for k in all_keys.iter() {
            trie.put(k, "value");
        }

        let have = trie.traverse();
        assert_eq!(have.len(), 6);

        let mut want = all_keys.clone();
        want.push("key");
        want.sort();

        for (i, w) in want.iter().enumerate() {
            assert_eq!(have[i].0, w.to_string());
        }
    }

    #[test]
    fn test_trie_put_new_key_returns_none() {
        let mut trie = TrieMap::new();
        assert!(trie.put("abc", "def").is_none());
    }

    #[test]
    fn test_trie_put_existing_key_returns_old_value() {
        let mut trie = TrieMap::new();
        trie.put("abc", "def");
        let old = trie.put("abc", "xyz");
        assert_eq!(old, Some(KVPair("abc".to_string(), "def".to_string())));
    }

    #[test]
    fn test_trie_put_prefix_keys_returns_none() {
        let mut trie = TrieMap::new();
        trie.put("abc", "def");
        let old = trie.put("ab", "value");
        assert!(old.is_none());
    }

    #[test]
    fn test_scan_prefix_range() {
        let mut trie = TrieMap::new();
        trie.put("ab", "v1");
        trie.put("abc", "v2");
        trie.put("abd", "v3");
        trie.put("ac", "v4");
        trie.put("zzz", "v5");

        // prefix "ab": canonical range end is the successor "ac"
        let have = trie.scan("ab", Some("ac"));
        let keys: Vec<_> = have.iter().map(|kv| kv.0.as_str()).collect();
        assert_eq!(keys, vec!["ab", "abc", "abd"]);
    }

    #[test]
    fn test_scan_exclusive_upper_bound() {
        let mut trie = TrieMap::new();
        trie.put("ab", "v1");
        trie.put("abc", "v2");
        trie.put("abd", "v3");
        trie.put("ac", "v4");

        let have = trie.scan("ab", Some("abd"));
        let keys: Vec<_> = have.iter().map(|kv| kv.0.as_str()).collect();
        assert_eq!(keys, vec!["ab", "abc"]);
    }

    #[test]
    fn test_scan_open_ended() {
        let mut trie = TrieMap::new();
        trie.put("ab", "v1");
        trie.put("abc", "v2");
        trie.put("ac", "v3");
        trie.put("zzz", "v4");

        let have = trie.scan("ac", None);
        let keys: Vec<_> = have.iter().map(|kv| kv.0.as_str()).collect();
        assert_eq!(keys, vec!["ac", "zzz"]);
    }

    #[test]
    fn test_scan_prunes_outside_range() {
        let mut trie = TrieMap::new();
        for k in ["aa", "ab", "abc", "ad", "b", "ba", "bb"] {
            trie.put(k, "v");
        }

        let have = trie.scan("ab", Some("ad"));
        let keys: Vec<_> = have.iter().map(|kv| kv.0.as_str()).collect();
        assert_eq!(keys, vec!["ab", "abc"]);

        let have = trie.scan("b", Some("bc"));
        let keys: Vec<_> = have.iter().map(|kv| kv.0.as_str()).collect();
        assert_eq!(keys, vec!["b", "ba", "bb"]);
    }

    #[test]
    fn test_scan_multibyte_utf8_range() {
        // Cyrillic keys are multi-byte in UTF-8: "a"..="z" analogs here are
        // \u{0430}..\u{044F}, all two-byte sequences starting with 0xD0/0xD1.
        let mut trie = TrieMap::new();
        trie.put("аб", "v1"); // a-b
        trie.put("ав", "v2"); // a-v
        trie.put("яб", "v3"); // ya-b, past the upper bound

        let have = trie.scan("а", Some("я"));
        let keys: Vec<_> = have.iter().map(|kv| kv.0.as_str()).collect();
        assert_eq!(keys, vec!["аб", "ав"]);
    }

    #[test]
    fn test_delete_prefix_only_deletes_matching_keys() {
        let mut trie = TrieMap::new();
        trie.put("ab", "v1");
        trie.put("abc", "v2");
        trie.put("ac", "v3");
        trie.put("zzz", "v4");

        let to_delete = trie.scan("ab", Some("ac"));
        assert_eq!(to_delete.len(), 2);
        for kv in &to_delete {
            trie.delete(kv.0.as_str());
        }

        let remaining = trie.traverse();
        assert_eq!(remaining.len(), 2);
        assert_eq!(remaining[0].0, "ac");
        assert_eq!(remaining[1].0, "zzz");
    }
}
