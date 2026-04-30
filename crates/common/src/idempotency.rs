use sha2::{Digest, Sha256};

/// Compute a stable idempotency key for a (kind, canonical_input_json) pair.
pub fn compute_idempotency_key(kind: &str, input_json: &serde_json::Value) -> String {
    let canonical = canonical_json(input_json);
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update(b":");
    hasher.update(canonical.as_bytes());
    hex::encode(hasher.finalize())
}

fn canonical_json(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let entries: Vec<String> = keys
                .into_iter()
                .map(|k| {
                    // serde_json::Value serialisation is infallible for JSON-native types.
                    let key = serde_json::to_string(k)
                        .expect("serializing a JSON string key is infallible");
                    format!("{}:{}", key, canonical_json(&map[k]))
                })
                .collect();
            format!("{{{}}}", entries.join(","))
        }
        serde_json::Value::Array(arr) => {
            let entries: Vec<String> = arr.iter().map(canonical_json).collect();
            format!("[{}]", entries.join(","))
        }
        other => serde_json::to_string(other).expect("serializing a JSON Value is infallible"),
    }
}
