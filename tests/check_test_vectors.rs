use open_rpc::{OpenRpc, Schema};
use rstest::*;
use serde_json::json;
use serde_json::Value as JsonValue;

#[rstest]
#[case(include_str!("test-vectors/simple-math-example.json"))]
#[case(include_str!("test-vectors/empty-openrpc.json"))]
#[case(include_str!("test-vectors/petstore-openrpc.json"))]
#[case(include_str!("test-vectors/link-example-openrpc.json"))]
#[case(include_str!("test-vectors/metrics-openrpc.json"))]
#[case(include_str!("test-vectors/params-by-name-petstore-openrpc.json"))]
#[case(include_str!("test-vectors/petstore-expanded-openrpc.json"))]
fn test_check_test_vectors(#[case] spec: &str) {
    serde_json::from_str::<OpenRpc>(spec).unwrap();
}

#[rstest]
#[case(json!({ "$ref": "#/components/schemas/user" }))]
#[case(json!({ }))]
fn test_check_schema_parse(#[case] value: JsonValue) {
    let value_str = serde_json::to_string(&value).unwrap();

    let _: Schema = serde_json::from_str::<Schema>(&value_str).unwrap();
}
