from __future__ import annotations

import json
from pathlib import Path


def _contract() -> dict:
    return json.loads(Path("tests/contracts/response_normalization_cases.json").read_text(encoding="utf-8"))


def test_response_normalization_contract_cases():
    from everos_hermes.response_normalization import as_list, count_hits, response_data, response_summary

    contract = _contract()
    for case in contract["cases"]:
        response = case["response"]
        assert sorted(response_data(response).keys()) == case["expected_data_keys"], case["name"]
        assert count_hits(response) == case["expected_hit_count"], case["name"]
        assert response_summary(response) == case["expected_summary"], case["name"]

    for case in contract["as_list_cases"]:
        assert as_list(case["input"]) == case["expected"]
