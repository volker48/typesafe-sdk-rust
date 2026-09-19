"""Translate scenario inputs/observations through the installed public Python SDK."""

import json
import sys

from pydantic import BaseModel, ConfigDict
from typesafe_sdk import (
    Choice,
    Noul,
    RetryPolicy,
    Score,
    TypeSafeAPIConnectionError,
    TypeSafeAPIError,
    TypeSafeAPIResponseValidationError,
    TypeSafeAPITimeoutError,
    TypeSafeClient,
    TypeSafeError,
    TypeSafeRateLimitError,
)


class ExtensionAnswer(BaseModel):
    model_config = ConfigDict(strict=True)
    value: list[str]


class ExtensionResponse(BaseModel):
    model_config = ConfigDict(strict=True)
    model: str
    answers: dict[str, ExtensionAnswer]


def policy(raw):
    return RetryPolicy(**raw) if raw is not None else None


def metadata(response):
    return {
        "status": response.status_code,
        "headers": dict(response.headers),
        "raw_hex": response.content.hex(),
    }


def main():
    scenario = json.load(sys.stdin)
    config = scenario.get("config", {}).copy()
    config["base_url"] = scenario["base_url"]
    if "retry" in config:
        config["retry"] = policy(config["retry"])
    observations = []
    with TypeSafeClient(**config) as client:
        for call in scenario["calls"]:
            args = call.copy()
            operation = args.pop("operation", "system_one")
            if operation not in {"system_one", "list_models"}:
                raise ValueError(f"Unsupported operation: {operation}")
            observe_retry_after = args.pop("observe_retry_after", False)
            # Python already passes dictionary questions through to the API.
            raw_questions = args.pop("raw_questions", False)
            custom_response = args.pop("custom_response", False)
            if operation != "system_one" and (raw_questions or custom_response):
                raise ValueError("Extensions require system_one")
            if raw_questions and args.get("typed", False):
                raise ValueError("Raw and typed question modes are mutually exclusive")
            if custom_response:
                args["response_model"] = ExtensionResponse
            if args.pop("typed", False):
                args["questions"] = {
                    key: {"noul": Noul, "choice": Choice, "score": Score}[q["type"]](
                        **{k: v for k, v in q.items() if k != "type"}
                    )
                    for key, q in args["questions"].items()
                }
            if "retry" in args:
                args["retry"] = policy(args["retry"])
            try:
                result = (
                    client.models.list(**args)
                    if operation == "list_models"
                    else client.system_one(**args)
                )
                observation = {"ok": result.model_dump(mode="json")}
                # Standalone Pydantic models do not expose Python HTTP metadata.
                if not custom_response:
                    observation["metadata"] = metadata(result.raw_http_response)
                observations.append(observation)
            except TypeSafeAPIError as error:
                kind = (
                    type(error).__name__.removeprefix("TypeSafe").removesuffix("Error")
                )
                names = {
                    "API": "api",
                    "APIResponseValidation": "validation",
                    "BadRequest": "bad_request",
                    "Authentication": "authentication",
                    "PermissionDenied": "permission_denied",
                    "NotFound": "not_found",
                    "UnprocessableEntity": "unprocessable_entity",
                    "RateLimit": "rate_limit",
                    "InternalServer": "internal_server",
                }
                observations.append(
                    {
                        "error": names[kind],
                        "status": error.status,
                        "body": error.body,
                        "headers": dict(error.headers),
                        "request_id": error.request_id,
                        "field_path": error.field_path
                        if isinstance(error, TypeSafeAPIResponseValidationError)
                        else None,
                        "endpoint": error.endpoint.replace(
                            scenario["origin"], "<origin>"
                        )
                        if error.endpoint is not None
                        else None,
                    }
                )
                if observe_retry_after:
                    observations[-1]["retry_after_ms"] = (
                        error.retry_after_ms
                        if isinstance(error, TypeSafeRateLimitError)
                        else None
                    )
            except TypeSafeAPITimeoutError:
                observations.append({"error": "timeout"})
            except TypeSafeAPIConnectionError:
                observations.append({"error": "connection"})
            except TypeSafeError:
                observations.append({"error": "input"})
    json.dump(observations, sys.stdout, ensure_ascii=False)


if __name__ == "__main__":
    main()
