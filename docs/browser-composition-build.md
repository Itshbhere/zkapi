# Browser SDK ownership

OA Chat consumes `@openanonymity/zkapi-browser-sdk` as an immutable dependency
and owns its UI, payment-mode runtime, routing, and Vercel deployments.

This repository has no OA Chat submodule or browser-chat build. See
[SDK integration](../sdk/README.md) for the host API and asset packaging,
and [the local client guide](../zkapi-clientd/README.md) for the independent
local daemon.
