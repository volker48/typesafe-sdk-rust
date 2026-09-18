"""Checks on comparison rules, independent of either SDK implementation."""

from run import canonical, numbers


def test_missing_null_boolean_and_order_are_not_hidden():
    assert canonical({}) != canonical({"q": None})
    assert canonical({"q": True}) != canonical({"q": 1})
    assert canonical([1, 2]) != canonical([2, 1])
    assert canonical({"a": 1, "b": 2}) == canonical({"b": 2, "a": 1})


def test_numeric_tokens_do_not_round_through_binary_floats():
    assert numbers('{"x":1.000000000000000000000001}') != numbers('{"x":1}')
    assert numbers('{"x":9007199254740993}') != numbers('{"x":9007199254740992}')
    assert numbers('{"x":1.00}') == numbers('{"x":1e0}')
    assert numbers('{"x":-0.0}') != numbers('{"x":0}')


def test_request_header_duplicates_are_preserved():
    import sys

    from run import execute

    # This synthetic adapter tests the recorder, not either SDK's business logic.
    script = """
import http.client, json, sys
from urllib.parse import urlsplit
p = json.load(sys.stdin)
u = urlsplit(p["origin"])
c = http.client.HTTPConnection(u.hostname, u.port)
c.putrequest("POST", "/v1/systemone")
for k,v in [("User-Agent","typesafe-sdk/0.7.0"),("X-TypeSafe-SDK","typesafe-sdk/0.7.0"),("X-TypeSafe-Runtime","python/test"),("Content-Length","2"),("X-Custom","first"),("X-Custom","second")]:
    c.putheader(k,v)
c.endheaders(b"{}")
c.getresponse().read()
c.close()
print("[]")
"""
    result = execute(
        {"id": "recorder", "calls": [], "responses": [{"status": 200, "body": {}}]},
        [sys.executable, "-c", script],
        {},
        "python",
    )
    assert result["requests"][0]["header_values"]["x-custom"] == ["first", "second"]
