use super::*;

#[test]
fn provider_save_tool_scope_agent_posts_agent_endpoint() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("provider_agent_save");
    let (base_url, handle) =
        one_request_server(json!({"data":{"status":"queued","task_id":"task-agent"}}));
    use_home_dotenv(&home, &base_url);

    let provider = EverOSProvider::initialize(ProviderInit::for_test("sess-1", &home)).unwrap();
    let raw = provider
        .handle_tool_call(
            "everos_memory_save",
            json!({"content":"retry with timeout","scope":"agent","flush":false}),
        )
        .unwrap();
    let request = handle.join().unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();
    assert!(request.starts_with("POST /api/v1/memories/agent HTTP/1.1"));
    assert_eq!(response["scope"], "agent");
    assert_eq!(
        response["agent_visibility"]["agent_visibility_status"],
        "unchecked"
    );
    assert_eq!(response["agent_visibility"]["agent_raw_queued"], true);

    remove_env("HERMES_HOME");
}

#[test]
fn provider_sync_turn_capture_agent_memory_posts_personal_and_agent_endpoints() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("provider_agent_sync");
    let (base_url, handle) =
        n_request_server(json!({"data":{"status":"queued","task_id":"task-sync"}}), 2);
    use_home_dotenv(&home, &base_url);
    fs::write(
        home.join("everos.json"),
        json!({
            "auto_capture": true,
            "flush_after_turn": false,
            "capture_agent_memory": true,
            "agent_capture_mode": "parallel",
            "agent_flush_after_turn": false
        })
        .to_string(),
    )
    .unwrap();

    let provider = EverOSProvider::initialize(ProviderInit::for_test("sess-1", &home)).unwrap();
    provider
        .sync_turn(
            "please debug the timeout regression",
            "checked task status before retrying and fixed it",
            Some("sess-2"),
        )
        .unwrap();
    let requests = handle.join().unwrap();
    let paths: Vec<&str> = requests
        .iter()
        .map(|raw| raw.lines().next().unwrap_or(""))
        .collect();
    assert!(
        paths
            .iter()
            .any(|line| line.starts_with("POST /api/v1/memories "))
    );
    assert!(
        paths
            .iter()
            .any(|line| line.starts_with("POST /api/v1/memories/agent "))
    );
    let agent_body = requests
        .iter()
        .find(|raw| raw.starts_with("POST /api/v1/memories/agent "))
        .map(|raw| parse_http_body(raw))
        .unwrap();
    assert_eq!(agent_body["user_id"], "u1");
    assert_eq!(agent_body["session_id"], "sess-2");
    assert!(
        agent_body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("Task request:")
    );

    remove_env("HERMES_HOME");
}

#[test]
fn rust_context_engine_policy_trajectory_match_python_contract() {
    let config = json!({
        "max_context_chars": 12_000,
        "agent_recall": true,
        "include_recent_raw": true,
        "recent_raw_top_k": 4,
        "min_recall_query_chars": 8
    });
    assert_eq!(
        should_skip_recall("ok", "sess-1", &config),
        (true, "trivial_query".to_string())
    );
    assert_eq!(
        should_skip_recall("继续下一步实验", "sess-1", &config),
        (false, String::new())
    );
    assert_eq!(
        should_skip_capture("thanks", "done", "sess-1", &config),
        (true, "trivial_turn".to_string())
    );
    let cache_key = stable_query_key(" Debug   Cache ", "sess-1", &config);
    assert_eq!(cache_key.len(), 64);
    assert_eq!(
        cache_key,
        stable_query_key("debug cache", "sess-1", &config)
    );
}

#[test]
fn rust_context_assembler_renders_python_v2_sections_and_generic_agent_memory() {
    let cfg = ContextAssemblyConfig {
        max_context_chars: 12_000,
        profile_max_items: 3,
        agent_skills_max_items: 4,
        agent_cases_max_items: 4,
        episodic_max_items: 6,
        recent_raw_top_k: 4,
        min_score: 0.0,
    };
    let assembled = assemble_everos_context(
        Some(&json!({"data": {
            "profiles": [{"id":"profile-1","profile_data":{"explicit_info":["User verifies every phase"]},"score":1.0}],
            "agent_skills": [{"id":"skill-1","name":"timeout recovery","description":"poll status before retry","score":0.9}],
            "agent_memory": [{"id":"generic-1","task_intent":"debug cache","approach":"reuse cached result","score":0.8}],
            "episodes": [{"id":"episode-1","subject":"cache","summary":"Cache should avoid duplicate search","score":0.7}]
        }})),
        Some(
            &json!({"data": {"raw_messages": [{"id":"raw-1","role":"user","content":"recent raw clue","score":0.6}]}}),
        ),
        "debug cache",
        &cfg,
        "prefetch",
    );
    let text = assembled.text;
    assert!(text.starts_with("<everos-context version=\"2\" source=\"prefetch\">"));
    let profile_idx = text.find("<profile>").unwrap();
    let skill_idx = text.find("<agent_skills>").unwrap();
    let case_idx = text.find("<agent_cases>").unwrap();
    let episodic_idx = text.find("<episodic>").unwrap();
    let raw_idx = text.find("<recent_context>").unwrap();
    assert!(
        profile_idx < skill_idx
            && skill_idx < case_idx
            && case_idx < episodic_idx
            && episodic_idx < raw_idx
    );
    assert!(text.contains("User verifies every phase"));
    assert!(text.contains("timeout recovery: poll status before retry"));
    assert!(text.contains("[agent_memory] debug cache: reuse cached result"));
    assert!(text.contains("cache: Cache should avoid duplicate search [score=0.70]"));
    assert!(text.contains("user: recent raw clue"));
    assert_eq!(assembled.included_counts["recent_context"], 1);
}

#[test]
fn provider_prefetch_uses_v2_assembler_cache_agent_and_session_scoped_raw() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("provider_context_engine");
    let (base_url, handle) = sequenced_request_server(
        vec![
            json!({"data":{"profiles":[{"id":"profile-1","profile_data":{"explicit_info":["User verifies every phase"]},"score":1.0}],"episodes":[{"id":"episode-1","subject":"cache","summary":"Cache should avoid duplicate search","score":0.7}]}}),
            json!({"data":{"agent_cases":[{"id":"case-1","task_intent":"debug cache","approach":"reuse cached result","score":0.9}]}}),
            json!({"data":{"raw_messages":[{"id":"raw-1","role":"user","content":"recent raw clue","score":0.8}]}}),
        ],
        500,
    );
    use_home_dotenv(&home, &base_url);
    fs::write(
        home.join("everos.json"),
        json!({"agent_recall":true,"include_recent_raw":true,"prefetch_cache_ttl_seconds":90})
            .to_string(),
    )
    .unwrap();

    let provider = EverOSProvider::initialize(ProviderInit::for_test("sess-1", &home)).unwrap();
    let context = provider.prefetch("debug cache", Some("sess-2"));
    let cached = provider.prefetch("debug cache", Some("sess-2"));
    let requests = handle.join().unwrap();
    let bodies: Vec<Value> = requests.iter().map(|raw| parse_http_body(raw)).collect();

    assert_eq!(context, cached);
    assert!(context.starts_with("<everos-context version=\"2\" source=\"prefetch\">"));
    assert!(context.contains("recent raw clue"));
    assert_eq!(requests.len(), 3);
    assert_eq!(
        bodies[0]["memory_types"],
        json!(["episodic_memory", "profile"])
    );
    assert_eq!(bodies[1]["memory_types"], json!(["agent_memory"]));
    assert_eq!(bodies[2]["memory_types"], json!(["raw_message"]));
    assert_eq!(bodies[2]["filters"]["AND"][0]["session_id"], "sess-2");

    remove_env("HERMES_HOME");
}

#[test]
fn provider_pre_compress_and_session_end_capture_structured_trajectory_with_dedupe() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("provider_precompress");
    let (base_url, handle) = n_request_server(
        json!({"data":{"status":"queued","task_id":"task-agent"}}),
        2,
    );
    use_home_dotenv(&home, &base_url);
    fs::write(
        home.join("everos.json"),
        json!({"capture_agent_memory":true}).to_string(),
    )
    .unwrap();

    let provider = EverOSProvider::initialize(ProviderInit::for_test("sess-1", &home)).unwrap();
    let messages = vec![
        json!({"role":"user","timestamp":1,"content":"run diagnostics"}),
        json!({"role":"assistant","timestamp":2,"content":"","tool_calls":[{"id":"call-1","function":{"name":"diagnose"}}]}),
        json!({"role":"tool","timestamp":3,"tool_call_id":"call-1","content":"diagnostics ok"}),
        json!({"role":"assistant","timestamp":4,"content":"verified"}),
    ];

    let summary = provider.on_pre_compress(&messages).unwrap();
    provider.on_session_end(&messages).unwrap();
    let requests = handle.join().unwrap();
    let first_body = parse_http_body(&requests[0]);

    assert!(summary.contains("EverOS captured 4 agent trajectory messages for session sess-1"));
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("POST /api/v1/memories/agent "));
    assert!(requests[1].starts_with("POST /api/v1/memories/flush "));
    assert_eq!(first_body["messages"].as_array().unwrap().len(), 4);

    remove_env("HERMES_HOME");
}

#[test]
fn provider_delegation_writes_child_session_id_prefix_and_agent_flush() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("provider_delegation");
    let (base_url, handle) = n_request_server(
        json!({"data":{"status":"queued","task_id":"task-delegation"}}),
        2,
    );
    use_home_dotenv(&home, &base_url);
    fs::write(
        home.join("everos.json"),
        json!({"capture_agent_memory":true}).to_string(),
    )
    .unwrap();

    let provider = EverOSProvider::initialize(ProviderInit::for_test("sess-1", &home)).unwrap();
    provider
        .on_delegation(
            "investigate failing test",
            "fixed with a regression test",
            Some("child-42"),
        )
        .unwrap();
    let requests = handle.join().unwrap();
    let body = parse_http_body(&requests[0]);
    let assistant = &body["messages"][1];

    assert!(requests[0].starts_with("POST /api/v1/memories/agent "));
    assert!(requests[1].starts_with("POST /api/v1/memories/agent/flush "));
    assert_eq!(assistant["child_session_id"], "child-42");

    remove_env("HERMES_HOME");
}

#[test]
fn provider_session_end_still_flushes_personal_after_agent_write_error() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("provider_session_end_error");
    let (base_url, handle) = sequenced_status_request_server(
        vec![
            (500, json!({"error":"agent write failed"})),
            (200, json!({"ok":true})),
        ],
        800,
    );
    use_home_dotenv(&home, &base_url);
    fs::write(
        home.join("everos.json"),
        json!({"capture_agent_memory":true}).to_string(),
    )
    .unwrap();

    let provider = EverOSProvider::initialize(ProviderInit::for_test("sess-err", &home)).unwrap();
    provider
        .on_session_end(&[json!({"role":"assistant","content":"final summary"})])
        .unwrap();
    let requests = handle.join().unwrap();

    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("POST /api/v1/memories/agent "));
    assert!(requests[1].starts_with("POST /api/v1/memories/flush "));

    remove_env("HERMES_HOME");
}
