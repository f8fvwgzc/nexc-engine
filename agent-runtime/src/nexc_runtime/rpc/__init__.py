"""The gRPC wire to the backend (CONTRACT section 8): `AgentRuntime/Execute`, one
bidirectional stream per node attempt, served with grpclib beside the FastAPI app. The
generated modules (`runtime_pb2`, `runtime_grpc`) come from the vendored proto through
`scripts/gen_proto.py`; `convert` maps messages to the request and event objects the runner
already works with, and `server` is the service."""

from .server import AgentRuntimeService, serve

__all__ = ["AgentRuntimeService", "serve"]
