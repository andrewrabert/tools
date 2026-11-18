//! YAML read into JSON values and written in block style.
use anyhow::Result;
use serde::Serialize;
use serde_json::{Map, Value};

/// The document in `text`; an empty document is an empty mapping.
pub fn load(text: &str) -> Result<Value> {
    let value: Value = serde_saphyr::from_str(text)?;
    Ok(match value {
        Value::Null => Value::Object(Map::new()),
        value => value,
    })
}

/// Block style with nothing folded: a long line stays one line.
pub fn dump<T: Serialize>(data: &T) -> Result<String> {
    let options = serde_saphyr::ser_options! {
        folded_wrap_chars: usize::MAX,
    };
    Ok(serde_saphyr::to_string_with_options(data, options)?)
}

pub fn dump_sorted(value: &Value) -> Result<String> {
    dump(&sorted(value.clone()))
}

pub fn sorted(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.cmp(b));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, sorted(value)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        value => value,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn empty_document_is_an_empty_mapping() {
        assert_eq!(load("").unwrap(), json!({}));
        assert_eq!(load("# only a comment\n").unwrap(), json!({}));
    }

    #[test]
    fn dump_sorted_sorts_nested_keys_and_keeps_lists() {
        let value = json!({"b": {"z": 1, "a": [3, 1, 2]}, "a": "x\ny\n", "c": []});
        assert_eq!(
            dump_sorted(&value).unwrap(),
            "a: |\n  x\n  y\nb:\n  a:\n  - 3\n  - 1\n  - 2\n  z: 1\nc: []\n"
        );
    }

    #[test]
    fn strings_that_read_as_other_types_are_quoted() {
        let value = json!({"a": "1", "b": "true", "c": "", "d": "null"});
        assert_eq!(
            dump_sorted(&value).unwrap(),
            "a: \"1\"\nb: \"true\"\nc: \"\"\nd: \"null\"\n"
        );
        assert_eq!(load(&dump_sorted(&value).unwrap()).unwrap(), value);
    }
}
