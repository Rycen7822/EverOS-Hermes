from __future__ import annotations

import json
from pathlib import Path


def _contract() -> dict:
    return json.loads(Path("tests/contracts/response_normalization_cases.json").read_text(encoding="utf-8"))


def test_response_normalization_contract_cases():
    from everos_hermes.response_normalization import response_summary

    contract = _contract()
    for case in contract["cases"]:
        response = case["response"]
        assert response_summary(response) == case["expected_summary"]
