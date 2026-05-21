use super::*;

#[test]
fn formatting_renders_episode_and_profile_context() {
    let context = format_search_context(
        &json!({
            "data": {
                "episodes": [{"subject":"coffee preference","summary":"User prefers strong black Americano.","score":0.91}],
                "profiles": [{"profile_data":{"explicit_info":["User likes black coffee"],"implicit_traits":["Prefers concise recommendations"]}}],
                "raw_messages": [{"role":"user","content":"raw request"}],
                "agent_memory": {"cases":[{"task_intent":"debug timeout","approach":"check task status before retry"}],"skills":[{"name":"timeout recovery","description":"poll task status"}]}
            }
        }),
        5,
    );
    assert!(context.contains("# EverOS Memory"));
    assert!(context.contains("coffee preference"));
    assert!(context.contains("raw request"));
    assert!(context.contains("timeout recovery"));
}
#[test]
fn tool_schema_snapshots_match_contracts() {
    assert_eq!(
        provider_schema_snapshot(),
        snapshot_json("provider_tools.snapshot.json")
    );
    assert_eq!(
        mcp_schema_snapshot(),
        snapshot_json("mcp_tools.snapshot.json")
    );
}

#[test]
fn provider_config_contract_clamps_drift_prone_fields() {
    let contract = snapshot_json("provider_config_contract.json");
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

    assert!(!defaults.agent_visibility_verify_after_write);
    assert!(!defaults.agent_visibility_verify_after_flush);
    assert!(defaults.agent_visibility_queries.is_empty());
    let home = temp_home("provider_visibility_config");
    fs::write(
        home.join("everos.json"),
        json!({"agent_visibility_verify_after_write": true, "agent_visibility_verify_after_flush": true, "agent_visibility_queries": "alpha, beta", "agent_visibility_top_k": 99}).to_string(),
    )
    .unwrap();
    let loaded = load_config(&home);
    assert!(loaded.agent_visibility_verify_after_write);
    assert!(loaded.agent_visibility_verify_after_flush);
    assert_eq!(loaded.agent_visibility_queries, vec!["alpha", "beta"]);
    assert_eq!(loaded.agent_visibility_top_k, 20);
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
    remove_env("HERMES_HOME");
}
