from __future__ import annotations

from everos_hermes.policy import should_skip_capture, should_skip_recall, stable_query_key


def test_skip_empty_and_temp_internal_sessions():
    assert should_skip_recall("", session_id="sess-1", config={}) == (True, "empty_query")
    assert should_skip_recall("remember this", session_id="temp:123", config={}) == (True, "temporary_session")
    assert should_skip_recall("remember this", session_id="internal:job", config={}) == (True, "internal_session")
    assert should_skip_capture("", "answer", session_id="sess-1", config={}) == (True, "empty_turn")
    assert should_skip_capture("question", "", session_id="sess-1", config={}) == (True, "empty_turn")


def test_skip_only_trivial_short_recall():
    assert should_skip_recall("ok", session_id="sess-1", config={}) == (True, "trivial_query")
    assert should_skip_recall("thanks", session_id="sess-1", config={}) == (True, "trivial_query")
    assert should_skip_recall("coffee preference", session_id="sess-1", config={}) == (False, "")
    assert should_skip_capture("ok", "done", session_id="sess-1", config={}) == (True, "trivial_turn")


def test_does_not_skip_chinese_continue_next_step():
    for query in ["继续", "下一步", "继续下一步实验"]:
        assert should_skip_recall(query, session_id="sess-1", config={}) == (False, "")
        assert should_skip_capture(query, "好的", session_id="sess-1", config={}) == (False, "")


def test_stable_query_key_changes_when_relevant_config_changes():
    base = stable_query_key(" Coffee Preference ", session_id="sess-1", config={"max_context_chars": 12000})
    same = stable_query_key("coffee preference", session_id="sess-1", config={"max_context_chars": 12000})
    changed_session = stable_query_key("coffee preference", session_id="sess-2", config={"max_context_chars": 12000})
    changed_config = stable_query_key("coffee preference", session_id="sess-1", config={"max_context_chars": 8000})

    assert base == same
    assert base != changed_session
    assert base != changed_config
