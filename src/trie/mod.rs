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
            new_node.put(full_key, &key_end[1..], value);
            e.insert(new_node);
            
            Some(KVPair(full_key.to_string(), value.to_string()))
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
}
