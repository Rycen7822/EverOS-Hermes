use serde_json::{Value, json};

pub fn tool_definitions() -> Vec<Value> {
    let mut tools = vec![
        json!({
            "name": "everos_save_memory",
            "title": "Save EverOS Memory",
            "description": "Queue one explicit text memory message for EverOS extraction. saved=true means accepted, not immediately searchable.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string"
                    },
                    "user_id": {
                        "type": "string"
                    },
                    "session_id": {
                        "type": "string"
                    },
                    "scope": {
                        "type": "string",
                        "enum": [
                            "personal",
                            "agent"
                        ],
                        "default": "personal"
                    },
                    "role": {
                        "type": "string",
                        "enum": [
                            "user",
                            "assistant",
                            "tool",
                            "system"
                        ]
                    },
                    "tool_call_id": {
                        "type": "string",
                        "description": "Required when role=tool for agent memory."
                    },
                    "flush": {
                        "type": "boolean",
                        "default": true
                    },
                    "async_mode": {
                        "type": "boolean",
                        "default": true
                    },
                    "flush_timeout": {
                        "type": "number"
                    }
                },
                "required": [
                    "content"
                ]
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": false,
                "openWorldHint": true
            }
        }),
        json!({
            "name": "everos_add_memories",
            "title": "Add EverOS Memory Messages",
            "description": "Add one or more personal or agent-trajectory messages to EverOS. Prefer scope over deprecated agent alias.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "messages": {
                        "type": "array",
                        "items": {
                            "type": "object"
                        }
                    },
                    "user_id": {
                        "type": "string"
                    },
                    "session_id": {
                        "type": "string"
                    },
                    "scope": {
                        "type": "string",
                        "enum": [
                            "personal",
                            "agent"
                        ],
                        "default": "personal"
                    },
                    "async_mode": {
                        "type": "boolean",
                        "default": true
                    },
                    "agent": {
                        "type": "boolean"
                    },
                    "flush": {
                        "type": "boolean",
                        "default": false
                    },
                    "flush_timeout": {
                        "type": "number"
                    }
                },
                "required": [
                    "messages"
                ]
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": false,
                "openWorldHint": true
            }
        }),
        json!({
            "name": "everos_flush_memories",
            "title": "Flush EverOS Memories",
            "description": "Trigger EverOS boundary detection and memory extraction immediately. Timeout errors are retryable; search/status checks should happen before retrying.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "user_id": {
                        "type": "string"
                    },
                    "session_id": {
                        "type": "string"
                    },
                    "scope": {
                        "type": "string",
                        "enum": [
                            "personal",
                            "agent"
                        ],
                        "default": "personal"
                    },
                    "agent": {
                        "type": "boolean"
                    },
                    "timeout": {
                        "type": "number"
                    }
                }
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": true
            }
        }),
        json!({
            "name": "everos_search_memories",
            "title": "Search EverOS Memories",
            "description": "Search EverOS memory using keyword, vector, hybrid, or agentic retrieval. Vector fields are stripped by default even when include_original_data=true; set include_vectors=true only for debugging.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string"
                    },
                    "user_id": {
                        "type": "string"
                    },
                    "session_id": {
                        "type": "string"
                    },
                    "filters": {
                        "type": "object"
                    },
                    "method": {
                        "type": "string",
                        "enum": [
                            "keyword",
                            "vector",
                            "hybrid",
                            "agentic"
                        ],
                        "default": "hybrid"
                    },
                    "top_k": {
                        "type": "integer",
                        "default": 5,
                        "minimum": -1,
                        "maximum": 100
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
                    "radius": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 1
                    },
                    "include_original_data": {
                        "type": "boolean",
                        "default": false
                    },
                    "include_vectors": {
                        "type": "boolean",
                        "default": false
                    },
                    "response_format": {
                        "type": "string",
                        "enum": [
                            "json",
                            "markdown"
                        ],
                        "default": "json"
                    },
                    "timeout": {
                        "type": "number"
                    },
                    "fallback_to_hybrid": {
                        "type": "boolean",
                        "default": true
                    }
                },
                "required": [
                    "query"
                ]
            },
            "annotations": {
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": true
            }
        }),
        json!({
            "name": "everos_get_memories",
            "title": "Get EverOS Memories",
            "description": "Retrieve structured EverOS memories by memory_type with pagination. get supports agent_case/agent_skill; search uses agent_memory.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "user_id": {
                        "type": "string"
                    },
                    "session_id": {
                        "type": "string"
                    },
                    "filters": {
                        "type": "object"
                    },
                    "memory_type": {
                        "type": "string",
                        "enum": [
                            "episodic_memory",
                            "profile",
                            "agent_case",
                            "agent_skill"
                        ],
                        "default": "episodic_memory"
                    },
                    "page": {
                        "type": "integer",
                        "default": 1
                    },
                    "page_size": {
                        "type": "integer",
                        "default": 20
                    },
                    "rank_by": {
                        "type": "string",
                        "default": "timestamp"
                    },
                    "rank_order": {
                        "type": "string",
                        "enum": [
                            "asc",
                            "desc"
                        ],
                        "default": "desc"
                    },
                    "response_format": {
                        "type": "string",
                        "enum": [
                            "json",
                            "markdown"
                        ],
                        "default": "json"
                    }
                }
            },
            "annotations": {
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": true
            }
        }),
        json!({
            "name": "everos_delete_memories",
            "title": "Delete EverOS Memories",
            "description": "Delete EverOS memory by exact memory_id, or batch-delete by user/session when explicitly confirmed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "memory_id": {
                        "type": "string"
                    },
                    "user_id": {
                        "type": "string"
                    },
                    "session_id": {
                        "type": "string"
                    },
                    "confirm": {
                        "type": "boolean",
                        "default": false
                    },
                    "confirm_scope_text": {
                        "type": "string"
                    }
                }
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": true,
                "idempotentHint": true,
                "openWorldHint": true
            }
        }),
        json!({
            "name": "everos_get_task_status",
            "title": "Get EverOS Task Status",
            "description": "Check an asynchronous EverOS extraction task status.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string"
                    }
                },
                "required": [
                    "task_id"
                ]
            },
            "annotations": {
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": true
            }
        }),
        json!({
            "name": "everos_get_settings",
            "title": "Get EverOS Settings",
            "description": "Get current EverOS memory-space settings.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            },
            "annotations": {
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": true
            }
        }),
        json!({
            "name": "everos_update_settings",
            "title": "Update EverOS Settings",
            "description": "Update EverOS memory-space settings.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "settings": {
                        "type": "object"
                    },
                    "strict": {
                        "type": "boolean",
                        "default": true
                    },
                    "return_diff": {
                        "type": "boolean",
                        "default": true
                    }
                },
                "required": [
                    "settings"
                ]
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": true
            }
        }),
        json!({
            "name": "everos_verify_session_ingest",
            "title": "Verify EverOS Session Ingest",
            "description": "Verify that an existing user/session is searchable by running read-only sample queries.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "verification_queries": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        }
                    },
                    "user_id": {
                        "type": "string"
                    },
                    "session_id": {
                        "type": "string"
                    },
                    "scope": {
                        "type": "string",
                        "enum": [
                            "personal",
                            "agent"
                        ],
                        "default": "personal"
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
                        "default": 5,
                        "minimum": -1,
                        "maximum": 100
                    },
                    "timeout": {
                        "type": "number"
                    }
                },
                "required": [
                    "verification_queries"
                ]
            },
            "outputSchema": {
                "type": "object",
                "properties": {
                    "ok": {
                        "type": "boolean"
                    },
                    "workflow": {
                        "type": "string"
                    },
                    "status": {
                        "type": "string"
                    },
                    "retryable": {
                        "type": "boolean"
                    },
                    "suggested_next_actions": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        }
                    }
                }
            },
            "annotations": {
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": true
            }
        }),
        json!({
            "name": "everos_save_and_verify",
            "title": "Save and Verify EverOS Memory",
            "description": "Queue one memory message, optionally flush, then verify searchability with sample queries.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string"
                    },
                    "verification_query": {
                        "type": "string"
                    },
                    "verification_queries": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        }
                    },
                    "user_id": {
                        "type": "string"
                    },
                    "session_id": {
                        "type": "string"
                    },
                    "scope": {
                        "type": "string",
                        "enum": [
                            "personal",
                            "agent"
                        ],
                        "default": "personal"
                    },
                    "role": {
                        "type": "string",
                        "enum": [
                            "user",
                            "assistant",
                            "tool",
                            "system"
                        ]
                    },
                    "tool_call_id": {
                        "type": "string",
                        "description": "Required when role=tool for agent memory."
                    },
                    "flush": {
                        "type": "boolean",
                        "default": true
                    },
                    "flush_timeout": {
                        "type": "number"
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
                        "default": 5,
                        "minimum": -1,
                        "maximum": 100
                    },
                    "timeout": {
                        "type": "number"
                    }
                },
                "required": [
                    "content"
                ]
            },
            "outputSchema": {
                "type": "object",
                "properties": {
                    "ok": {
                        "type": "boolean"
                    },
                    "workflow": {
                        "type": "string"
                    },
                    "status": {
                        "type": "string"
                    },
                    "retryable": {
                        "type": "boolean"
                    },
                    "suggested_next_actions": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        }
                    }
                }
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": false,
                "openWorldHint": true
            }
        }),
        json!({
            "name": "everos_import_and_verify",
            "title": "Import and Verify EverOS Memories",
            "description": "Batch-import messages or a local file, then flush/poll-compatible verify with sample queries.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "messages": {
                        "type": "array",
                        "items": {
                            "type": "object"
                        }
                    },
                    "file_path": {
                        "type": "string"
                    },
                    "verification_queries": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        }
                    },
                    "user_id": {
                        "type": "string"
                    },
                    "session_id": {
                        "type": "string"
                    },
                    "scope": {
                        "type": "string",
                        "enum": [
                            "personal",
                            "agent"
                        ],
                        "default": "personal"
                    },
                    "dry_run": {
                        "type": "boolean",
                        "default": false
                    },
                    "batch_size": {
                        "type": "integer",
                        "default": 50,
                        "minimum": 1,
                        "maximum": 100
                    },
                    "flush": {
                        "type": "boolean",
                        "default": true
                    },
                    "flush_timeout": {
                        "type": "number"
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
                        "default": 5,
                        "minimum": -1,
                        "maximum": 100
                    },
                    "timeout": {
                        "type": "number"
                    }
                }
            },
            "outputSchema": {
                "type": "object",
                "properties": {
                    "ok": {
                        "type": "boolean"
                    },
                    "workflow": {
                        "type": "string"
                    },
                    "status": {
                        "type": "string"
                    },
                    "retryable": {
                        "type": "boolean"
                    },
                    "suggested_next_actions": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        }
                    }
                }
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": false,
                "openWorldHint": true
            }
        }),
    ];
    let output_schema = text_result_output_schema();
    for tool in &mut tools {
        match tool.as_object_mut() {
            Some(map) if !map.contains_key("outputSchema") => {
                map.insert("outputSchema".to_string(), output_schema.clone());
            }
            _ => {}
        }
    }
    tools
}

fn text_result_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "result": {
                "type": "string"
            }
        },
        "required": ["result"]
    })
}
