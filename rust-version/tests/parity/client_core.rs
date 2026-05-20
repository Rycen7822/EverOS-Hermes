use super::*;

#[test]
fn dotenv_lookup_prefers_process_env_then_hermes_home_file() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("dotenv");
    fs::write(
        home.join(".env"),
        "# comment\nexport EVEROS_API_KEY='dotenv-key'\nEVEROS_BASE_URL=https://everos.example.test/ # comment\nEVEROS_TIMEOUT=3\n",
    )
    .unwrap();
    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_BASE_URL");
    set_env("HERMES_HOME", home.to_str().unwrap());

    let parsed = read_dotenv(&home.join(".env"));
    assert_eq!(parsed.get("EVEROS_API_KEY").unwrap(), "dotenv-key");
    assert_eq!(get_env("EVEROS_API_KEY", "", Some(&home)), "dotenv-key");
    set_env("EVEROS_API_KEY", "process-key");
    assert_eq!(get_env("EVEROS_API_KEY", "", Some(&home)), "process-key");

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_BASE_URL");
    remove_env("HERMES_HOME");
}

#[test]
fn client_posts_bearer_json_to_add_memories() {
    let (base_url, handle) =
        one_request_server(json!({"data":{"status":"queued","task_id":"task-1"}}));
    let client = EverOSClient::new("test-key", &base_url, 7.0).unwrap();
    let response = client
        .add_memories(
            "user_001",
            Some("session_001"),
            vec![json!({"role":"user","timestamp":1711900000000_i64,"content":"I like black coffee."})],
            true,
            false,
        )
        .unwrap();
    let raw = handle.join().unwrap();
    let body = parse_http_body(&raw);

    assert_eq!(response["data"]["task_id"], "task-1");
    assert!(raw.starts_with("POST /api/v1/memories HTTP/1.1"));
    assert!(
        raw.contains("authorization: Bearer test-key")
            || raw.contains("Authorization: Bearer test-key")
    );
    assert_eq!(body["user_id"], "user_001");
    assert_eq!(body["session_id"], "session_001");
    assert_eq!(body["async_mode"], true);
    assert_eq!(body["messages"][0]["content"], "I like black coffee.");
}

#[test]
fn client_search_uses_hybrid_defaults_and_session_filter() {
    let (base_url, handle) = one_request_server(json!({"data":{"episodes":[]}}));
    let client = EverOSClient::new("test-key", &base_url, 10.0).unwrap();
    client
        .search_memories(
            "coffee preference",
            Some("user_001"),
            Some("session_001"),
            None,
            "hybrid",
            None,
            5,
            None,
            false,
            false,
            None,
        )
        .unwrap();
    let raw = handle.join().unwrap();
    let body = parse_http_body(&raw);

    assert!(raw.starts_with("POST /api/v1/memories/search HTTP/1.1"));
    assert_eq!(body["query"], "coffee preference");
    assert_eq!(body["filters"]["user_id"], "user_001");
    assert_eq!(body["filters"]["AND"][0]["session_id"], "session_001");
    assert_eq!(body["method"], "hybrid");
    assert_eq!(body["memory_types"], json!(DEFAULT_MEMORY_TYPES));
    assert_eq!(body["top_k"], 5);
    assert_eq!(body.get("radius"), None);
}

#[test]
fn client_search_strips_vectors_by_default_but_can_keep_them() {
    let payload = json!({
        "data": {
            "episodes": [{"id":"ep1","summary":"Coffee","vector":[0.1,0.2]}],
            "original_data": {"episodes": {"ep1": {"vector":[0.1,0.2],"embedding":[0.3]}}}
        }
    });

    let stripped = strip_vectors(&payload);
    assert!(stripped.to_string().contains("Coffee"));
    assert!(!stripped.to_string().contains("vector"));
    assert!(!stripped.to_string().contains("embedding"));

    let (base_url, handle) = one_request_server(payload.clone());
    let client = EverOSClient::new("test-key", &base_url, 10.0).unwrap();
    let response = client
        .search_memories(
            "coffee",
            Some("user_001"),
            None,
            None,
            "hybrid",
            None,
            5,
            None,
            true,
            false,
            None,
        )
        .unwrap();
    handle.join().unwrap();
    assert!(response.to_string().contains("Coffee"));
    assert!(!response.to_string().contains("vector"));

    let (base_url, handle) = one_request_server(payload);
    let client = EverOSClient::new("test-key", &base_url, 10.0).unwrap();
    let response = client
        .search_memories(
            "coffee",
            Some("user_001"),
            None,
            None,
            "hybrid",
            None,
            5,
            None,
            true,
            true,
            None,
        )
        .unwrap();
    handle.join().unwrap();
    assert_eq!(response["data"]["episodes"][0]["vector"], json!([0.1, 0.2]));
}
