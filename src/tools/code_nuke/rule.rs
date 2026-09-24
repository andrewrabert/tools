use serde_json::{Value, json};

pub fn drop_stmt(pattern: &str, kinds: &[String]) -> Value {
    let kinds: Vec<Value> = kinds.iter().map(|kind| json!({ "kind": kind })).collect();
    json!({
        "id": "drop-stmt",
        "language": "rust",
        "utils": {
            "is-stmt": { "any": kinds },
            "holds-call": { "has": { "pattern": pattern, "stopBy": "end" } },
        },
        "rule": {
            "all": [
                { "matches": "is-stmt" },
                { "matches": "holds-call" },
                {
                    "not": {
                        "has": {
                            "all": [{ "matches": "is-stmt" }, { "matches": "holds-call" }],
                            "stopBy": "end",
                        },
                    },
                },
            ],
        },
        "fix": "",
    })
}

pub fn drop_tests() -> Value {
    json!({
        "id": "drop-tests",
        "language": "rust",
        "utils": {
            "test-attr": {
                "kind": "attribute_item",
                "regex": r"^#\[\s*(cfg\(test\)|([a-z_]+::)*test)\s*\]$",
            },
            "attr-run": {
                "any": [
                    { "matches": "test-attr" },
                    { "kind": "attribute_item", "follows": { "matches": "test-attr" } },
                ],
            },
        },
        "rule": {
            "any": [
                { "kind": "mod_item", "follows": { "matches": "attr-run" } },
                { "kind": "function_item", "follows": { "matches": "attr-run" } },
                { "matches": "attr-run" },
            ],
        },
        "fix": "",
    })
}
