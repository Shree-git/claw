# Stability reference

This page summarizes what can change in the `v0.1.x` line. The source of truth
for public surfaces is [Public Interface Manifest](public-interface-manifest.md).

## Stability levels

| Level | Meaning | Change rule |
|---|---|---|
| Stable | Intended for production use inside the current major line. | Breaking changes need a deprecation window. |
| Beta | Ready for controlled trials. | Minor releases can change behavior with release-note callouts. |
| Experimental | Still being shaped. | Can change or be removed in any release. |
| Reserved | Held for future compatibility. | Must not be reused for another meaning. |
| Internal | Implementation detail. | Can change without public migration guidance. |

## Current surfaces

| Surface | Level |
|---|---|
| CLI command surface | Experimental |
| Successful CLI JSON schemas | Experimental, versioned |
| Operational evidence reports | Experimental, versioned |
| Policy schema | Experimental |
| Daemon HTTP health and metrics v1 | Beta |
| gRPC sync protocol | Experimental |
| Plugin protocol v1 | Experimental, versioned |
| Git interop contract | Experimental |
| Internal Rust crate APIs | Not public |
| Temporary files and caches | Not public |

Field-level object and protocol tiers are recorded in
[Object and Protocol Stability Tiers](object-stability-tiers.md).

Claw VCS is still pre-1.0. The manifest documents the public surfaces we test
and try to move deliberately, but v0.1.x does not provide a compatibility
guarantee for CLI behavior, policy semantics, object formats, daemon APIs, or
sync protocol behavior. Pin exact versions for automation and read release
notes before upgrading.

## Release rules

- Stable removals follow the deprecation policy once a surface is promoted to
  stable.
- Beta breaking changes must be named in release notes.
- Experimental changes should still include migration notes when users are likely
  to depend on the behavior.
- Debug logs and tracing span names are not public contracts unless a doc says so.
