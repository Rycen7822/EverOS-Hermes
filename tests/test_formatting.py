from everos_hermes.formatting import format_search_context, strip_vectors


def test_format_search_context_renders_nested_agent_memory_cases_and_skills():
    context = format_search_context(
        {
            "data": {
                "agent_memory": {
                    "cases": [{"task_intent": "debug timeout", "approach": "check task status before retry"}],
                    "skills": [{"name": "MCP timeout recovery", "description": "Search/status before retry."}],
                }
            }
        },
        max_items=5,
    )

    assert "## Agent Cases" in context
    assert "debug timeout" in context
    assert "check task status before retry" in context
    assert "## Agent Skills" in context
    assert "MCP timeout recovery" in context


def test_strip_vectors_removes_nested_embeddings_from_agent_memory():
    cleaned = strip_vectors(
        {
            "data": {
                "agent_memory": {
                    "cases": [{"summary": "case", "embedding": [1, 2, 3]}],
                    "skills": [{"name": "skill", "vector": [0.1]}],
                }
            }
        }
    )

    rendered = str(cleaned)
    assert "case" in rendered
    assert "skill" in rendered
    assert "embedding" not in rendered
    assert "vector" not in rendered



def test_format_search_context_profile_fallback_is_per_item():
    context = format_search_context(
        {
            "data": {
                "profiles": [
                    {"profile_data": {"facts": ["known fact"]}},
                    {"id": "p2", "profile_data": {"new_schema": [{"value": "should fallback"}]}},
                ]
            }
        },
        max_items=5,
    )

    assert "known fact" in context
    assert "new_schema" in context
    assert "should fallback" in context
