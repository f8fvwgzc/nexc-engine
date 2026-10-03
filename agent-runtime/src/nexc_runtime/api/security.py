"""Bearer-token auth and request size limiting."""

from __future__ import annotations

import hmac
import json

from fastapi import HTTPException, Request, status
from starlette.types import ASGIApp, Message, Receive, Scope, Send

from ..config import Settings


def require_bearer(request: Request) -> None:
    """FastAPI dependency: constant-time check of `Authorization: Bearer <NEXC_RUNTIME_TOKEN>`."""
    settings: Settings = request.app.state.settings
    expected = settings.nexc_runtime_token.get_secret_value().encode("utf-8")
    header = request.headers.get("authorization", "")
    scheme, _, presented = header.partition(" ")
    ok = scheme.lower() == "bearer" and hmac.compare_digest(
        presented.strip().encode("utf-8"), expected
    )
    if not ok:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="invalid or missing bearer token",
            headers={"WWW-Authenticate": "Bearer"},
        )


class BodySizeLimit:
    """Pure ASGI middleware rejecting bodies above `max_bytes` with 413 (header and stream)."""

    def __init__(self, app: ASGIApp, max_bytes: int) -> None:
        self.app = app
        self.max_bytes = max_bytes

    async def __call__(self, scope: Scope, receive: Receive, send: Send) -> None:
        if scope["type"] != "http":
            await self.app(scope, receive, send)
            return

        for name, value in scope.get("headers", []):
            if name == b"content-length":
                try:
                    declared = int(value)
                except ValueError:
                    declared = -1
                if declared < 0 or declared > self.max_bytes:
                    await _reject(send, self.max_bytes)
                    return

        received = 0
        response_started = False

        async def limited_receive() -> Message:
            nonlocal received
            message = await receive()
            if message["type"] == "http.request":
                received += len(message.get("body", b""))
                if received > self.max_bytes:
                    raise _TooLarge
            return message

        async def tracking_send(message: Message) -> None:
            nonlocal response_started
            if message["type"] == "http.response.start":
                response_started = True
            await send(message)

        try:
            await self.app(scope, limited_receive, tracking_send)
        except _TooLarge:
            if not response_started:
                await _reject(send, self.max_bytes)


class _TooLarge(Exception):
    pass


async def _reject(send: Send, limit: int) -> None:
    body = json.dumps({"detail": f"request body exceeds {limit} bytes"}).encode()
    await send(
        {
            "type": "http.response.start",
            "status": 413,
            "headers": [
                (b"content-type", b"application/json"),
                (b"content-length", str(len(body)).encode()),
            ],
        }
    )
    await send({"type": "http.response.body", "body": body})
