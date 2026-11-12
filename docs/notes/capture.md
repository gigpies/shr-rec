# Capture readiness and identity boundary

Standalone capture deliberately wraps the existing raw engine rather than making
JACK or terminal state part of the GigPies host ABI. The common Session owns the
exclusive transport; preparation, attachment, callback enablement and durable
completion are separate facts. Recording stops before its producer moves back to
an owner/finalizer. Retired observer faults must never stop a later playback owner.

JACK port names are persistent preferences, while UUIDs identify the current
discovered instance. Replacing a port under an identical name requires a fresh
explicit REFRESH/inspection; it cannot silently reuse stale startup eligibility.
The take's optional identity sidecar preserves ordered stable IDs and selected
UUIDs without altering raw v1. Copy recovery reads optional identity metadata
under the same descriptor/lock boundary used to validate stems before creation;
the public raw-host recovery path remains independent of standalone metadata.

[Owning task](../PLAN.md#sp-04--task0029-standalone-capture) ·
[Capture engineering contract](../CAPTURE.md).
