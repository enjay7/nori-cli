//! JSON-RPC 2.0 protocol types for Agent Context Protocol

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 Request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<Value>,
    pub id: Value,
}

/// JSON-RPC 2.0 Response
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub result: Option<Value>,
    pub error: Option<JsonRpcError>,
    pub id: Value,
}

/// JSON-RPC 2.0 Notification (no id field)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<Value>,
}

/// JSON-RPC 2.0 Error
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn test_json_rpc_request_serialization() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "session/new".to_string(),
            params: Some(json!({"foo": "bar"})),
            id: json!(1),
        };

        let serialized = serde_json::to_string(&request).unwrap();
        let deserialized: JsonRpcRequest = serde_json::from_str(&serialized).unwrap();

        assert_eq!(request, deserialized);
        assert_eq!(deserialized.jsonrpc, "2.0");
        assert_eq!(deserialized.method, "session/new");
    }

    #[test]
    fn test_json_rpc_response_with_result() {
        let response = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: Some(json!({"session_id": "abc123"})),
            error: None,
            id: json!(1),
        };

        let serialized = serde_json::to_string(&response).unwrap();
        let deserialized: JsonRpcResponse = serde_json::from_str(&serialized).unwrap();

        assert_eq!(response, deserialized);
        assert!(deserialized.result.is_some());
        assert!(deserialized.error.is_none());
    }

    #[test]
    fn test_json_rpc_response_with_error() {
        let response = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: "Method not found".to_string(),
                data: None,
            }),
            id: json!(1),
        };

        let serialized = serde_json::to_string(&response).unwrap();
        let deserialized: JsonRpcResponse = serde_json::from_str(&serialized).unwrap();

        assert_eq!(response, deserialized);
        assert!(deserialized.result.is_none());
        assert!(deserialized.error.is_some());
        assert_eq!(deserialized.error.unwrap().code, -32601);
    }

    #[test]
    fn test_json_rpc_notification_no_id() {
        let notification = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: "session/update".to_string(),
            params: Some(json!({"status": "in_progress"})),
        };

        let serialized = serde_json::to_string(&notification).unwrap();
        let deserialized: JsonRpcNotification = serde_json::from_str(&serialized).unwrap();

        assert_eq!(notification, deserialized);
        // Verify serialized JSON doesn't contain "id" field
        assert!(!serialized.contains("\"id\""));
    }
}
