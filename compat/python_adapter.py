"""Translate scenario inputs/observations through the installed public Python SDK."""

import json
import sys

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
)


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
                result = client.system_one(**args)
                observations.append(
                    {
                        "ok": result.model_dump(mode="json"),
                        "metadata": metadata(result.raw_http_response),
                    }
                )
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
            except TypeSafeAPITimeoutError:
                observations.append({"error": "timeout"})
            except TypeSafeAPIConnectionError:
                observations.append({"error": "connection"})
    json.dump(observations, sys.stdout, ensure_ascii=False)


if __name__ == "__main__":
    main()
