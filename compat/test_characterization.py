"""Additional public-API characterizations; upstream source/assertions are untouched."""

import httpx2
import pytest
from pydantic import ValidationError
from typesafe_sdk import Noul, RetryPolicy, Score, SystemOneResponse


def test_one_score_level_is_accepted_despite_api_markdown():
    assert Score(criteria=["only"]).model_dump() == {
        "type": "score",
        "criteria": ["only"],
    }


def test_typed_none_is_omitted_but_nested_none_is_preserved():
    assert Noul(instructions=None, criteria={"true": None}).model_dump() == {
        "type": "noul",
        "criteria": {"true": None},
    }


def test_bool_retry_count_is_accepted_as_python_integer():
    assert RetryPolicy(max_retries=True).max_retries is True


def test_response_counts_are_unbounded_python_integers():
    response = SystemOneResponse.from_http_response(
        httpx2.Response(200, json={"model": "m", "usage": {"input_tokens": 2**80}})
    )
    assert response.usage.input_tokens == 2**80
    assert response.answers == {}


def test_unknown_fields_on_typed_questions_are_rejected():
    with pytest.raises(ValidationError):
        Noul.model_validate({"unexpected": True})


def test_score_key_coercion_accepts_decimal_integer_spelling():
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
                        "legend": {"0.0": "zero"},
                        "probabilities": {"0.0": 1},
                    }
                },
            },
        )
    )
    assert response.scores["q"].legend == {0: "zero"}
    assert response.scores["q"].probabilities == {0: 1.0}
