"""Entry point: `nexc-runtime` or `python -m nexc_runtime`."""

from __future__ import annotations

import sys

import uvicorn
from pydantic import ValidationError

from .api.app import create_app
from .config import Settings, get_settings
from .logging_setup import configure_logging


def load_settings() -> Settings:
    try:
        return get_settings()
    except ValidationError as exc:
        # Report which settings are wrong without echoing their (possibly secret) values.
        print("nexc-runtime: invalid configuration:", file=sys.stderr)
        for err in exc.errors(include_input=False, include_url=False):
            field = ".".join(str(p) for p in err["loc"]).upper()
            print(f"  {field}: {err['msg']}", file=sys.stderr)
        print("Run `make init` at the repo root to generate secrets.", file=sys.stderr)
        raise SystemExit(2) from None


def main() -> None:
    settings = load_settings()
    configure_logging(settings.runtime_log_level)
    settings.runtime_workspace.mkdir(parents=True, exist_ok=True, mode=0o700)
    uvicorn.run(
        create_app(settings),
        host=settings.runtime_host,
        port=settings.runtime_port,
        log_config=None,
        server_header=False,
        proxy_headers=False,
        timeout_graceful_shutdown=30,
    )


if __name__ == "__main__":
    main()
