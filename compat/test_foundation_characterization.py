"""Pin intentional numeric differences without changing the upstream tests."""

import math

import httpx2
import pytest
from typesafe_sdk import SystemOneResponse, TypeSafeRateLimitError


@pytest.mark.parametrize("literal", ["NaN", "Infinity", "-Infinity"])
def test_python_accepts_nonstandard_response_numbers(literal):
    response = SystemOneResponse.from_http_response(
        httpx2.Response(
            200,
            content=(
                '{"model":"m","usage":{},"answers":{"q":{"type":"noul","noul":'
                + literal
                + "}}}"
            ),
        )
    )
    value = response.nouls["q"].noul
    assert math.isnan(value) if literal == "NaN" else math.isinf(value)


@pytest.mark.parametrize("key", ["9007199254740993.0", "9223372036854775808.0"])
def test_python_score_key_conversion_is_exact_and_unbounded(key):
    response = SystemOneResponse.from_http_response(
        httpx2.Response(
            200,
            json={
                "model": "m",
                "usage": {},
                "answers": {
                    "q": {
                        "type": "score",
                        "score": 0,
                        "confidence": 1,
                        "legend": {key: "rubric"},
                        "probabilities": {key: 1},
                    }
                },
            },
        )
    )
    assert response.scores["q"].legend == {int(key.removesuffix(".0")): "rubric"}


def test_large_millisecond_delay_takes_precedence_over_seconds():
    error = TypeSafeRateLimitError(
        429, {}, httpx2.Headers({"retry-after-ms": "1e30", "retry-after": "2"})
    )
    assert error.retry_after_ms == 1e30
