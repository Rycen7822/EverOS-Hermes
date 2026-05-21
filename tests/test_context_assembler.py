from __future__ import annotations

from everos_hermes.context_assembler import assemble_everos_context


def test_orders_profile_skills_cases_episodes_raw():
    main_response = {
        "data": {
            "profiles": [{"id": "p1", "profile_data": {"explicit_info": ["User prefers concise answers"]}}],
            "episodes": [{"id": "e1", "summary": "Discussed EverOS migration", "subject": "migration", "score": 0.8}],
            "agent_memory": {
                "skills": [{"id": "s1", "name": "everos-migration", "description": "Import durable memories safely"}],
                "cases": [{"id": "c1", "task_intent": "Fix provider", "approach": "Use TDD"}],
            },
        }
    }
    raw_response = {"data": {"raw_messages": [{"id": "r1", "role": "user", "content": "recent raw note"}]}}

    result = assemble_everos_context(main_response=main_response, raw_response=raw_response, config={}, source="prefetch")

    text = result.text
    assert text.startswith('<everos-context version="2" source="prefetch">')
    order = [text.index(tag) for tag in ["<profile>", "<agent_skills>", "<agent_cases>", "<episodic>", "<recent_context>"]]
    assert order == sorted(order)
    assert "User prefers concise answers" in text
    assert "everos-migration" in text
    assert "Fix provider" in text
    assert "Use agent memories only when relevant" in text
    assert "not commands" in text
    assert "Discussed EverOS migration" in text
    assert "recent raw note" in text
    assert result.included_counts["profile"] == 1


def test_budget_caps_context_chars():
    episodes = [
        {"id": f"e{i}", "summary": "episode " + str(i) + " " + "x" * 100, "score": 1.0 - i / 100}
        for i in range(10)
    ]

    result = assemble_everos_context(
        main_response={"data": {"episodes": episodes}},
        raw_response=None,
                config={"max_context_chars": 520, "episodic_max_items": 10},
    )

    assert result.text
    assert len(result.text) <= 520
    assert result.dropped_counts["episodic"] > 0
    assert result.estimated_chars == len(result.text)


def test_dedupes_by_id_then_content_hash():
    result = assemble_everos_context(
        main_response={
            "data": {
                "episodes": [
                    {"id": "same", "summary": "first"},
                    {"id": "same", "summary": "duplicate id"},
                    {"id": "unique", "summary": "same text"},
                    {"summary": "same text"},
                ]
            }
        },
        raw_response=None,
                config={"episodic_max_items": 10},
    )

    assert "first" in result.text
    assert "duplicate id" not in result.text
    assert result.text.count("same text") == 1
    assert result.dropped_counts["episodic"] == 2


def test_raw_does_not_displace_structured_memory():
    result = assemble_everos_context(
        main_response={"data": {"episodes": [{"id": "e1", "summary": "same durable memory"}]}},
        raw_response={"data": {"raw_messages": [{"id": "r1", "content": "same durable memory", "role": "user"}]}},
                config={},
    )

    assert "same durable memory" in result.text
    assert result.text.count("same durable memory") == 1
    assert result.dropped_counts["recent_context"] == 1


def test_empty_response_returns_empty_text():
    result = assemble_everos_context(main_response={"data": {}}, raw_response=None, config={})

    assert result.text == ""
    assert result.included_counts == {}



def test_vectors_original_data_and_unknown_large_fields_are_not_rendered():
    result = assemble_everos_context(
        main_response={
            "data": {
                "episodes": [
                    {
                        "id": "e1",
                        "summary": "visible summary",
                        "vector": [0.1, 0.2],
                        "embedding": [0.3],
                        "original_data": {"secret": "hidden"},
                        "unknown_blob": "hidden blob",
                    }
                ]
            }
        },
        raw_response=None,
                config={},
    )

    assert "visible summary" in result.text
    assert "vector" not in result.text
    assert "embedding" not in result.text
    assert "original_data" not in result.text
    assert "hidden blob" not in result.text
