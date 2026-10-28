"""Entry point: `nexc-runtime` or `python -m nexc_runtime`.

Serves the HTTP API (uvicorn, `RUNTIME_PORT`) and the gRPC stream (grpclib,
`RUNTIME_GRPC_PORT`, 0 turns it off) in one event loop; they share the census, so the
capacity limit counts runs of both wires.
"""

from __future__ import annotations

import asyncio
import logging
import sys

import uvicorn
from pydantic import ValidationError

from .api.app import create_app
from .config import Settings, get_settings
from .logging_setup import configure_logging
from .rpc import serve as serve_grpc

log = logging.getLogger(__name__)


def load_settings() -> Settings:
    try:
        return get_settings()
    except ValidationError as exc:
        # Report which settings are wrong without echoing their (possibly secret) values.
        print("nexc-runtime: invalid configuration:", file=sys.stderr)
        for err in exc.errors(include_input=False, include_url=False):
            field = ".".join(str(p) for p in err["loc"]).upper() or "SETTINGS"
            print(f"  {field}: {err['msg']}", file=sys.stderr)
        print("Run `make init` at the repo root to generate secrets.", file=sys.stderr)
        raise SystemExit(2) from None


async def run(settings: Settings) -> None:
    app = create_app(settings)
    config = uvicorn.Config(
        app,
        host=settings.runtime_host,
        port=settings.runtime_port,
        log_config=None,
        server_header=False,
        proxy_headers=False,
        timeout_graceful_shutdown=30,
    )
    http = uvicorn.Server(config)
    grpc = None
    if settings.runtime_grpc_port:
        grpc = await serve_grpc(settings, app.state.census, keys=app.state.keys)
        log.info(
            "gRPC stream listening on %s:%s (%s)",
            settings.runtime_host,
            settings.runtime_grpc_port,
            "TLS" if settings.runtime_grpc_tls_cert_file else "plaintext",
        )
    try:
        await http.serve()
    finally:
        if grpc is not None:
            grpc.close()
            await grpc.wait_closed()


def main() -> None:
    settings = load_settings()
    configure_logging(settings.runtime_log_level)
    settings.runtime_workspace.mkdir(parents=True, exist_ok=True, mode=0o700)
    asyncio.run(run(settings))


if __name__ == "__main__":
    main()
