from everos_hermes.formatting import format_search_context


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
