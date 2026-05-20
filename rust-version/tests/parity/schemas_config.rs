use super::*;

#[test]
fn formatting_renders_episode_and_profile_context() {
    let context = format_search_context(
        &json!({
            "data": {
                "episodes": [{"subject":"coffee preference","summary":"User prefers strong black Americano.","score":0.91}],
                "profiles": [{"profile_data":{"explicit_info":["User likes black coffee"],"implicit_traits":["Prefers concise recommendations"]}}]
            }
        }),
        5,
    );
    assert!(context.contains("# EverOS Memory"));
    assert!(context.contains("coffee preference"));
    assert!(context.contains("strong black Americano"));
    assert!(context.contains("User likes black coffee"));
}

#[test]
fn agent_visibility_workflow_status_mapping_is_stable() {
    use everos_hermes_rust::agent_visibility::workflow_status_from_agent_visibility;

    assert_eq!(
        workflow_status_from_agent_visibility(
            &json!({"agent_visibility_status":"visible"}),
            "fallback"
        ),
        "verified"
    );
    assert_eq!(
        workflow_status_from_agent_visibility(
            &json!({"agent_visibility_status":"partial"}),
            "fallback"
        ),
        "partially_verified"
    );
    assert_eq!(
        workflow_status_from_agent_visibility(
            &json!({"agent_visibility_status":"not_visible"}),
            "fallback"
        ),
        "agent_not_visible"
    );
    assert_eq!(
        workflow_status_from_agent_visibility(
            &json!({"agent_visibility_status":"error"}),
            "fallback"
        ),
        "agent_visibility_error"
    );
    assert_eq!(
        workflow_status_from_agent_visibility(
            &json!({"agent_visibility_status":"unchecked"}),
            "fallback"
        ),
        "fallback"
    );
}

#[test]
fn provider_tool_schemas_match_snapshot() {
    assert_eq!(
        provider_schema_snapshot(),
        snapshot_json("provider_tools.snapshot.json")
    );
}

#[test]
fn mcp_tool_schemas_match_snapshot() {
    assert_eq!(
        mcp_schema_snapshot(),
        snapshot_json("mcp_tools.snapshot.json")
    );
}

#[test]
fn provider_config_contract_clamps_drift_prone_fields() {
    let contract = provider_config_contract();
    let fields = contract["fields"].as_object().unwrap();
    let defaults = ProviderConfig::default();
    for (key, spec) in fields {
        assert_eq!(
            provider_config_usize_field(&defaults, key),
            spec["default"].as_u64().unwrap() as usize,
            "default for {key}"
        );
    }

    let home = temp_home("provider_config_contract_min");
    let below_min = Value::Object(
        fields
            .iter()
            .filter(|(_, spec)| spec["min"].as_u64().unwrap() > 0)
            .map(|(key, _)| (key.clone(), json!(0)))
            .collect(),
    );
    fs::write(home.join("everos.json"), below_min.to_string()).unwrap();
    let loaded = load_config(&home);
    for (key, spec) in fields {
        if spec["min"].as_u64().unwrap() > 0 {
            assert_eq!(
                provider_config_usize_field(&loaded, key),
                spec["min"].as_u64().unwrap() as usize,
                "min clamp for {key}"
            );
        }
    }

    let home = temp_home("provider_config_contract_max");
    let above_max = Value::Object(
        fields
            .iter()
            .map(|(key, spec)| (key.clone(), json!(spec["max"].as_u64().unwrap() + 1)))
            .collect(),
    );
    fs::write(home.join("everos.json"), above_max.to_string()).unwrap();
    let loaded = load_config(&home);
    for (key, spec) in fields {
        assert_eq!(
            provider_config_usize_field(&loaded, key),
            spec["max"].as_u64().unwrap() as usize,
            "max clamp for {key}"
        );
    }
}

#[test]
fn provider_agent_visibility_config_defaults_and_load_overrides() {
    let defaults = ProviderConfig::default();
    assert!(!defaults.agent_visibility_verify_after_write);
    assert!(!defaults.agent_visibility_verify_after_flush);
    assert!(defaults.agent_visibility_queries.is_empty());
    assert_eq!(defaults.agent_visibility_top_k, 5);
    assert_eq!(defaults.agent_visibility_timeout, 30.0);
    assert_eq!(defaults.agent_visibility_get_page_size, 20);
    assert_eq!(defaults.agent_visibility_retry_flush_attempts, 1);

    let home = temp_home("provider_visibility_config");
    fs::write(
        home.join("everos.json"),
        json!({
            "agent_visibility_verify_after_write": true,
            "agent_visibility_verify_after_flush": true,
            "agent_visibility_queries": "alpha, beta",
            "agent_visibility_top_k": 99,
            "agent_visibility_timeout": 0.1,
            "agent_visibility_get_page_size": 200,
            "agent_visibility_retry_flush_attempts": 9
        })
        .to_string(),
    )
    .unwrap();
    let loaded = load_config(&home);
    assert!(loaded.agent_visibility_verify_after_write);
    assert!(loaded.agent_visibility_verify_after_flush);
    assert_eq!(loaded.agent_visibility_queries, vec!["alpha", "beta"]);
    assert_eq!(loaded.agent_visibility_top_k, 20);
    assert_eq!(loaded.agent_visibility_timeout, 1.0);
    assert_eq!(loaded.agent_visibility_get_page_size, 100);
    assert_eq!(loaded.agent_visibility_retry_flush_attempts, 5);
}

#[test]
fn provider_availability_user_resolution_and_tool_schemas_match_python_surface() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("provider");
    fs::write(
        home.join(".env"),
        "EVEROS_API_KEY=test-key\nEVEROS_USER_ID=hermes_{identity}_{platform}\n",
    )
    .unwrap();
    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    set_env("HERMES_HOME", home.to_str().unwrap());

    assert!(EverOSProvider::is_available(Some(&home)));
    let provider = EverOSProvider::initialize(ProviderInit {
        session_id: "sess-1".into(),
        hermes_home: Some(home.clone()),
        platform: "telegram".into(),
        user_id: "tg-42".into(),
        user_name: "Xu".into(),
        agent_identity: "default".into(),
        agent_context: "".into(),
    })
    .unwrap();

    assert_eq!(provider.name(), "everos");
    assert_eq!(provider.user_id(), "hermes_default_telegram");
    assert!(
        provider
            .system_prompt_block()
            .contains("everos_memory_search")
    );
    let tool_names: Vec<String> = provider
        .tool_schemas()
        .iter()
        .map(|schema| schema["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        tool_names,
        vec![
            "everos_memory_save",
            "everos_memory_search",
            "everos_memory_get",
            "everos_memory_flush",
            "everos_memory_forget",
            "everos_memory_save_and_verify",
            "everos_memory_import_and_verify",
            "everos_memory_verify_session",
        ]
    );

    remove_env("HERMES_HOME");
}
