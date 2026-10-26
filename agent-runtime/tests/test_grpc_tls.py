"""TLS on the gRPC stream, which carries the LLM keys of workspaces that bring their own: with
RUNTIME_GRPC_TLS_* the runtime serves it over TLS and, with a client CA, answers only a backend
that presents a certificate of it. The PKI is tests/fixtures/grpc-tls."""

from __future__ import annotations

import socket
import ssl
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest
from grpclib.client import Channel
from pydantic import ValidationError

from nexc_runtime.agents import Census
from nexc_runtime.config import Settings
from nexc_runtime.rpc import runtime_pb2 as pb
from nexc_runtime.rpc.runtime_grpc import AgentRuntimeStub
from nexc_runtime.rpc.server import serve

from .conftest import AUTH, execute_body
from .test_grpc import collect, kinds, to_request

PKI = Path(__file__).parent / "fixtures" / "grpc-tls"


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port: int = sock.getsockname()[1]
        return port


def client_context(*, ca: str = "ca.pem", certificate: bool = True) -> ssl.SSLContext:
    context = ssl.create_default_context(cafile=PKI / ca)
    context.set_alpn_protocols(["h2"])
    if certificate:
        context.load_cert_chain(PKI / "client.pem", PKI / "client-key.pem")
    return context


async def run_demo(port: int, context: ssl.SSLContext | None) -> list[str]:
    """A demo run over a channel to the runtime; the kinds of the events it streamed."""
    body = execute_body(llm={"provider": "demo", "api_key": None})
    async with Channel("127.0.0.1", port, ssl=context) as channel:
        stub = AgentRuntimeStub(channel)
        async with stub.Execute.open(metadata=[("authorization", AUTH["Authorization"])]) as stream:
            await stream.send_message(pb.BackendCommand(execute=to_request(body)), end=True)
            return kinds(await collect(stream))


@pytest.mark.asyncio
async def test_the_stream_is_served_over_tls_to_a_backend_with_a_certificate(
    make_settings: Callable[..., Settings],
) -> None:
    settings = make_settings(
        runtime_grpc_tls_cert_file=PKI / "server.pem",
        runtime_grpc_tls_key_file=PKI / "server-key.pem",
        runtime_grpc_tls_client_ca_file=PKI / "ca.pem",
    )
    port = free_port()
    server = await serve(settings, Census(), host="127.0.0.1", port=port)
    try:
        seen = await run_demo(port, client_context())
        assert seen[-1] == "result", seen
        # Without a client certificate, trusting another CA, or in plaintext: no run.
        refused: list[ssl.SSLContext | None] = [
            client_context(certificate=False),
            client_context(ca="other-ca.pem"),
            None,
        ]
        for context in refused:
            with pytest.raises(Exception):  # noqa: B017, PT011 - TLS, h2 or stream errors alike
                await run_demo(port, context)
    finally:
        server.close()
        await server.wait_closed()


@pytest.mark.asyncio
async def test_without_a_certificate_the_stream_stays_plaintext(
    make_settings: Callable[..., Settings],
) -> None:
    settings = make_settings()
    assert settings.grpc_ssl_context() is None
    port = free_port()
    server = await serve(settings, Census(), host="127.0.0.1", port=port)
    try:
        assert (await run_demo(port, None))[-1] == "result"
    finally:
        server.close()
        await server.wait_closed()


@pytest.mark.parametrize(
    "overrides",
    [
        {"runtime_grpc_tls_cert_file": PKI / "server.pem"},
        {"runtime_grpc_tls_key_file": PKI / "server-key.pem"},
        {"runtime_grpc_tls_client_ca_file": PKI / "ca.pem"},
        {
            "runtime_grpc_tls_cert_file": PKI / "missing.pem",
            "runtime_grpc_tls_key_file": PKI / "server-key.pem",
        },
    ],
)
def test_tls_settings_go_together(
    make_settings: Callable[..., Settings], overrides: dict[str, Any]
) -> None:
    with pytest.raises(ValidationError):
        make_settings(**overrides)
