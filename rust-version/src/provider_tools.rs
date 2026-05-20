use serde_json::{Value, json};

pub fn provider_tool_schemas() -> Vec<Value> {
    vec![
        json!({
            "name": "everos_memory_save",
            "description": "Queue an explicit long-term memory message in EverOS and optionally request extraction; saved=true does not guarantee a structured memory is immediately searchable. Agent scope returns agent_visibility=unchecked unless verification is enabled.",
            "parameters": {
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "Memory content to store."
                    },
                    "session_id": {
                        "type": "string",
                        "description": "Optional EverOS/Hermes session id."
                    },
                    "scope": {
                        "type": "string",
                        "enum": [
                            "personal",
                            "agent"
                        ],
                        "description": "Memory scope. Default personal."
                    },
                    "role": {
                        "type": "string",
                        "enum": [
                            "user",
                            "assistant",
                            "tool",
                            "system"
                        ],
                        "description": "Message role. role=tool is only valid with scope=agent and requires tool_call_id."
                    },
                    "tool_call_id": {
                        "type": "string",
                        "description": "Required when role=tool for agent memory."
                    },
                    "flush": {
                        "type": "boolean",
                        "description": "Trigger EverOS extraction immediately. Default true."
                    }
                },
                "required": [
                    "content"
                ]
            }
        }),
        json!({
            "name": "everos_memory_search",
            "description": "Search EverOS long-term memory using keyword, vector, hybrid, or agentic retrieval.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Backward-compatible alias for top_k."
                    },
                    "top_k": {
                        "type": "integer",
                        "description": "Cloud top_k; -1 requests all matching results."
                    },
                    "method": {
                        "type": "string",
                        "enum": [
                            "keyword",
                            "vector",
                            "hybrid",
                            "agentic"
                        ],
                        "description": "Retrieval method. Default hybrid."
                    },
                    "session_id": {
                        "type": "string",
                        "description": "Optional session filter."
                    },
                    "filters": {
                        "type": "object",
                        "description": "Optional Cloud v1 filters DSL. user_id is filled from provider config."
                    },
                    "memory_types": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "enum": [
                                "episodic_memory",
                                "profile",
                                "raw_message",
                                "agent_memory"
                            ]
                        },
                        "description": "Optional EverOS search memory types."
                    },
                    "radius": {
                        "type": "number",
                        "description": "Optional vector radius for vector/hybrid/agentic retrieval."
                    },
                    "include_original_data": {
                        "type": "boolean",
                        "description": "Include Cloud original_data. Vectors remain stripped by default."
                    },
                    "include_vectors": {
                        "type": "boolean",
                        "description": "Keep embedding/vector fields for debugging only."
                    },
                    "response_format": {
                        "type": "string",
                        "enum": [
                            "json",
                            "markdown"
                        ],
                        "description": "Output format."
                    }
                },
                "required": [
                    "query"
                ]
            }
        }),
        json!({
            "name": "everos_memory_get",
            "description": "Get structured EverOS memories by type for the configured user.",
            "parameters": {
                "type": "object",
                "properties": {
                    "memory_type": {
                        "type": "string",
                        "enum": [
                            "episodic_memory",
                            "profile",
                            "agent_case",
                            "agent_skill"
                        ],
                        "description": "Memory type to retrieve."
                    },
                    "page": {
                        "type": "integer",
                        "description": "Page number starting at 1."
                    },
                    "page_size": {
                        "type": "integer",
                        "description": "Items per page, 1-100."
                    },
                    "session_id": {
                        "type": "string",
                        "description": "Optional session filter."
                    },
                    "filters": {
                        "type": "object",
                        "description": "Optional Cloud v1 filters DSL. user_id is filled from provider config."
                    },
                    "rank_by": {
                        "type": "string",
                        "description": "Rank field. Default timestamp."
                    },
                    "rank_order": {
                        "type": "string",
                        "enum": [
                            "asc",
                            "desc"
                        ],
                        "description": "Rank order."
                    }
                }
            }
        }),
        json!({
            "name": "everos_memory_flush",
            "description": "Force EverOS memory extraction for the configured user/session. Timeout errors are retryable; search/status checks should happen before retrying. Agent scope reports flush separately from structured visibility and retries transient request-send failures once.",
            "parameters": {
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Optional session id."
                    },
                    "scope": {
                        "type": "string",
                        "enum": [
                            "personal",
                            "agent"
                        ],
                        "description": "Memory scope to flush."
                    },
                    "timeout": {
                        "type": "number",
                        "description": "Optional per-call timeout in seconds."
                    }
                }
            }
        }),
        json!({
            "name": "everos_memory_forget",
            "description": "Delete an EverOS memory by id. Requires confirm=true because this is destructive.",
            "parameters": {
                "type": "object",
                "properties": {
                    "memory_id": {
                        "type": "string",
                        "description": "Exact EverOS memory id to delete."
                    },
                    "confirm": {
                        "type": "boolean",
                        "description": "Must be true to delete."
                    }
                },
                "required": [
                    "memory_id",
                    "confirm"
                ]
            }
        }),
        json!({
            "name": "everos_memory_save_and_verify",
            "description": "Queue one memory message, optionally flush extraction, then verify searchability with sample queries. Agent scope returns structured agent_visibility status.",
            "parameters": {
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "Memory content to store."
                    },
                    "verification_query": {
                        "type": "string",
                        "description": "Primary query used to verify recall."
                    },
                    "verification_queries": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        },
                        "description": "Additional verification queries."
                    },
                    "session_id": {
                        "type": "string",
                        "description": "Optional session id."
                    },
                    "scope": {
                        "type": "string",
                        "enum": [
                            "personal",
                            "agent"
                        ],
                        "description": "Memory scope."
                    },
                    "role": {
                        "type": "string",
                        "enum": [
                            "user",
                            "assistant",
                            "tool",
                            "system"
                        ],
                        "description": "Message role; role=tool requires tool_call_id."
                    },
                    "tool_call_id": {
                        "type": "string",
                        "description": "Required when role=tool for agent memory."
                    },
                    "flush": {
                        "type": "boolean",
                        "description": "Trigger extraction before verification. Default true."
                    },
                    "top_k": {
                        "type": "integer",
                        "description": "Verification search top_k."
                    }
                },
                "required": [
                    "content"
                ]
            }
        }),
        json!({
            "name": "everos_memory_import_and_verify",
            "description": "Batch-import messages or a local file, with dry-run validation, optional flush, and verification report.",
            "parameters": {
                "type": "object",
                "properties": {
                    "messages": {
                        "type": "array",
                        "items": {
                            "type": "object"
                        },
                        "description": "Messages to import."
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Optional local JSON/JSONL/Markdown file to import."
                    },
                    "verification_queries": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        },
                        "description": "Queries used to verify recall."
                    },
                    "session_id": {
                        "type": "string",
                        "description": "Optional session id."
                    },
                    "scope": {
                        "type": "string",
                        "enum": [
                            "personal",
                            "agent"
                        ],
                        "description": "Memory scope."
                    },
                    "dry_run": {
                        "type": "boolean",
                        "description": "Validate and summarize without writing."
                    },
                    "batch_size": {
                        "type": "integer",
                        "description": "Messages per add_memories call."
                    },
                    "flush": {
                        "type": "boolean",
                        "description": "Flush after importing. Default true."
                    },
                    "top_k": {
                        "type": "integer",
                        "description": "Verification search top_k."
                    }
                }
            }
        }),
        json!({
            "name": "everos_memory_verify_session",
            "description": "Read-only verification for an existing user/session using sample search queries. Agent scope returns structured agent_visibility status.",
            "parameters": {
                "type": "object",
                "properties": {
                    "verification_queries": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        },
                        "description": "Queries used to verify recall."
                    },
                    "session_id": {
                        "type": "string",
                        "description": "Optional session id."
                    },
                    "scope": {
                        "type": "string",
                        "enum": [
                            "personal",
                            "agent"
                        ],
                        "description": "Memory scope."
                    },
                    "memory_types": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "enum": [
                                "episodic_memory",
                                "profile",
                                "raw_message",
                                "agent_memory"
                            ]
                        }
                    },
                    "top_k": {
                        "type": "integer",
                        "description": "Verification search top_k."
                    }
                },
                "required": [
                    "verification_queries"
                ]
            }
        }),
    ]
}
