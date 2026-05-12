from __future__ import annotations

import pytest


def test_search_validator_accepts_top_k_boundaries_and_agent_memory():
    from everos_hermes.schemas import validate_search_params

    for top_k in [-1, 0, 5, 100]:
        validate_search_params("hybrid", ["episodic_memory", "profile", "agent_memory"], top_k, 0.5)


def test_search_validator_rejects_invalid_top_k_radius_and_type():
    from everos_hermes.schemas import validate_search_params

    with pytest.raises(ValueError, match="top_k"):
        validate_search_params("hybrid", ["episodic_memory"], -2, None)
    with pytest.raises(ValueError, match="top_k"):
        validate_search_params("hybrid", ["episodic_memory"], 101, None)
    with pytest.raises(ValueError, match="radius"):
        validate_search_params("hybrid", ["episodic_memory"], 5, 1.1)
    with pytest.raises(ValueError, match="memory_types"):
        validate_search_params("hybrid", ["agent_case"], 5, None)
    with pytest.raises(ValueError, match="radius"):
        validate_search_params("keyword", ["episodic_memory"], 5, 0.5)


def test_get_validator_distinguishes_get_memory_types_and_rank_order():
    from everos_hermes.schemas import validate_get_params

    validate_get_params("agent_case", 1, 100, "timestamp", "DESC")
    with pytest.raises(ValueError, match="memory_type"):
        validate_get_params("agent_memory", 1, 20, "timestamp", "desc")
    with pytest.raises(ValueError, match="page_size"):
        validate_get_params("profile", 1, 101, "timestamp", "desc")
    with pytest.raises(ValueError, match="rank_order"):
        validate_get_params("profile", 1, 20, "timestamp", "newest")


def test_filters_require_user_and_reject_unknown_or_conflicting_fields():
    from everos_hermes.schemas import build_filters, validate_filters

    filters = build_filters(user_id="u1", session_id="s1", filters={"AND": [{"timestamp": {"gte": 1700000000000}}]})
    assert filters == {
        "user_id": "u1",
        "AND": [{"timestamp": {"gte": 1700000000000}}, {"session_id": "s1"}],
    }
    validate_filters(filters)

    with pytest.raises(ValueError, match="user_id"):
        validate_filters({"session_id": "s1"})
    with pytest.raises(ValueError, match="Unknown filter field"):
        validate_filters({"user_id": "u1", "group_id": "g1"})
    with pytest.raises(ValueError, match="conflict"):
        build_filters(user_id="u2", filters={"user_id": "u1"})
    with pytest.raises(ValueError, match="conflict"):
        build_filters(user_id="u1", session_id="s2", filters={"user_id": "u1", "session_id": "s1"})


def test_message_scope_and_delete_validators():
    from everos_hermes.schemas import validate_delete_request, validate_messages

    validate_messages([{"role": "user", "timestamp": 1711900000000, "content": "hello", "message_id": "msg-1"}], "personal")
    validate_messages(
        [{"role": "tool", "timestamp": 1711900000000, "content": "tool output", "tool_call_id": "tool-call-1"}],
        "agent",
    )
    with pytest.raises(ValueError, match="tool_call_id"):
        validate_messages([{"role": "tool", "timestamp": 1711900000000, "content": "tool output"}], "agent")
    with pytest.raises(ValueError, match="message_id"):
        validate_messages([{"role": "user", "timestamp": 1711900000000, "content": "hello", "message_id": ""}], "personal")
    with pytest.raises(ValueError, match="message_id"):
        validate_messages([{"role": "user", "timestamp": 1711900000000, "content": "hello", "message_id": 123}], "personal")
    with pytest.raises(ValueError, match="role"):
        validate_messages([{"role": "tool", "timestamp": 1, "content": "no"}], "personal")
    with pytest.raises(ValueError, match="1..500"):
        validate_messages([], "personal")
    with pytest.raises(ValueError, match="1..500"):
        validate_messages([{"role": "user", "timestamp": i, "content": "x"} for i in range(501)], "personal")

    validate_delete_request(memory_id="m1", user_id=None, session_id=None)
    validate_delete_request(memory_id=None, user_id="u1", session_id="s1")
    with pytest.raises(ValueError, match="single delete"):
        validate_delete_request(memory_id="m1", user_id="u1", session_id=None)
    with pytest.raises(ValueError, match="explicit user_id"):
        validate_delete_request(memory_id=None, user_id=None, session_id="s1")


def test_settings_validator_strict_timezone_and_llm_custom_setting():
    from everos_hermes.schemas import validate_settings_update

    assert validate_settings_update({"timezone": "Asia/Tokyo", "llm_custom_setting": {"style": "concise"}}) == {
        "timezone": "Asia/Tokyo",
        "llm_custom_setting": {"style": "concise"},
    }
    with pytest.raises(ValueError, match="Unknown settings"):
        validate_settings_update({"extraction_mode": "fast"})
    with pytest.raises(ValueError, match="timezone"):
        validate_settings_update({"timezone": "Tokyo"})
    with pytest.raises(ValueError, match="llm_custom_setting"):
        validate_settings_update({"llm_custom_setting": []})
