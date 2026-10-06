"""FastAPI application factory and routes."""

from __future__ import annotations

import asyncio
import contextlib
from collections.abc import AsyncIterator

from fastapi import Depends, FastAPI, HTTPException, Request, Response
from fastapi.exceptions import RequestValidationError
from fastapi.responses import JSONResponse, StreamingResponse

from .. import __version__
from ..agents import Census
from ..config import Settings
from ..parsing import ParseError, UnsupportedFormatError, parse_document
from ..runner import ProviderFactory, default_provider_factory, execute
from ..streaming import EventStream
from .schemas import ExecuteRequest, Health, ParseBlock, ParseResponse
from .security import BodySizeLimit, require_bearer

NDJSON = "application/x-ndjson"
PARSE_PATH = "/v1/parse"


def create_app(
    settings: Settings, *, provider_factory: ProviderFactory = default_provider_factory
) -> FastAPI:
    app = FastAPI(
        title="nexc agent runtime",
        version=__version__,
        docs_url=None,  # internal service: no public docs surface
        redoc_url=None,
        openapi_url=None,
    )
    app.state.settings = settings
    app.state.census = Census()
    app.state.provider_factory = provider_factory
    app.add_middleware(
        BodySizeLimit,
        max_bytes=settings.runtime_max_request_bytes,
        path_limits={PARSE_PATH: settings.runtime_max_parse_bytes},
    )

    @app.exception_handler(RequestValidationError)
    async def _validation_error(_: Request, exc: RequestValidationError) -> JSONResponse:
        # Never echo request input back: it may contain the per-request API key.
        errors = [
            {"loc": list(err.get("loc", ())), "msg": err.get("msg", ""), "type": err.get("type")}
            for err in exc.errors()
        ]
        return JSONResponse(status_code=422, content={"detail": errors})

    @app.get("/healthz", response_model=Health)
    async def healthz(request: Request) -> Health:
        census: Census = request.app.state.census
        return Health(version=__version__, agents_loaded=census.agents_live)

    @app.post("/v1/execute", dependencies=[Depends(require_bearer)], response_model=None)
    async def execute_node(
        body: ExecuteRequest, request: Request
    ) -> StreamingResponse | JSONResponse:
        census: Census = request.app.state.census
        if census.runs_active >= settings.runtime_max_concurrent_runs:
            return JSONResponse(
                status_code=429,
                content={"detail": "runtime is at capacity"},
                headers={"Retry-After": "5"},
            )
        return StreamingResponse(
            _stream(request.app, body),
            media_type=NDJSON,
            headers={"Cache-Control": "no-store", "X-Accel-Buffering": "no"},
        )

    @app.post(PARSE_PATH, dependencies=[Depends(require_bearer)], response_model=None)
    async def parse(request: Request, filename: str = "") -> Response:
        """Parse an uploaded file (the raw request body) into heading / text / table blocks."""
        data = await request.body()
        if len(data) > settings.runtime_max_parse_bytes:  # the middleware already enforces this
            raise HTTPException(status_code=413, detail="request body is too large")
        try:
            # CPU-bound: a worker thread keeps the event loop free for /v1/execute streams.
            payload = await asyncio.to_thread(_parse_to_json, filename, data)
        except UnsupportedFormatError as error:
            raise HTTPException(status_code=415, detail=str(error)) from error
        except ParseError as error:
            raise HTTPException(status_code=422, detail=str(error)) from error
        return Response(content=payload, media_type="application/json")

    return app


def _parse_to_json(filename: str, data: bytes) -> bytes:
    """Parse and serialise in one go, so neither step runs on the event loop."""
    document = parse_document(filename, data)
    response = ParseResponse(
        pages=document.pages,
        blocks=[
            ParseBlock(
                kind=block.kind,
                level=block.level,
                text=block.text,
                page=block.page,
                rows=block.rows,
                header_rows=block.header_rows,
            )
            for block in document.blocks
        ],
    )
    return response.model_dump_json().encode("utf-8")


async def _stream(app: FastAPI, body: ExecuteRequest) -> AsyncIterator[bytes]:
    events = EventStream()
    task = asyncio.create_task(
        execute(
            body,
            settings=app.state.settings,
            census=app.state.census,
            events=events,
            provider_factory=app.state.provider_factory,
        )
    )
    try:
        async for line in events.lines():
            yield line
        await task
    finally:
        if not task.done():  # client went away: stop the agents
            task.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await task
