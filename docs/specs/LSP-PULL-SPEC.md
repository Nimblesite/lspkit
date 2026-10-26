# [LSP-PULL] Pull Diagnostic Infrastructure

## [LSP-PULL-CAPABILITIES] Standard Capability Decoding

Initialization accepts the standard plural `workspace.diagnostics` capability.
Protocol-library spelling differences are normalized once at the decoding boundary.
Backend applications must not maintain a second capability parser.

## [LSP-PULL-SALSA] Diagnostic State Ownership

Salsa is the sole owner of computed diagnostic values. A session retains document
input handles and an input revision, never an independent result map. A backend
failure remains an error; it must not become an empty successful report.

An unchanged result identity avoids backend work. A computed report carries the
identity of the same snapshot that produced its diagnostics. Document versions
and workspace revisions invalidate the tracked query.

## [LSP-PULL-CONCURRENCY] Nonblocking Invalidation

Input revision changes must not wait behind a compiler query. Query execution
may be serialized around its Salsa database, but the foreground protocol loop
continues receiving edits, cancellation and shutdown. A superseded query cannot
be reported under the newer snapshot's identity or document version.

Supersession revisions and engine generations are independent lock-free inputs.
Editor, configuration, watched-file and workspace-selection changes advance both:
they invalidate Salsa and supersede in-flight work. A compiler engine change
advances only the generation, so an in-flight pull completes with the identity of
its captured `p:{revision}|d:{version}|g:{generation}` snapshot. The new generation
invalidates Salsa and gives the next pull a fresh identity; only a revision mismatch
supersedes the in-flight pull.

## [LSP-PULL-CANCELLATION] Request Lifetime

The shared dispatcher owns cancellation registration. A request is registered
before it is scheduled so cancellation cannot race ahead of registration.
Completion, cancellation and dropped work release that registration. Shutdown
cancels all outstanding requests. A workspace scan checks cancellation and its
snapshot revision between documents and before emitting partial or final results.

An ingress reader can register replaceable request interest before forwarding a
request to the foreground loop. Preparation adopts that same registration, so
cancellation received while queued remains effective. A changed input identity
supersedes older work only for the same method and canonical resource. The first
interruption wins: client cancellation remains `-32800`; supersession returns
`-32802` with `data.retriggerRequest = true`. Interest stores protocol metadata
only, and completion or dispatcher shutdown releases it with the sole registry.
Consumers can supersede a method/resource scope when actual input changes, even
if a client repeats its previous result identity. This uses the same registry
and first-interruption rule; it does not retain another input or result cache.
Replacing the consumer's complete context can supersede all registrations through
that same weak handle and registry. It preserves first-interruption semantics and
each request's completion guard; it does not restart the dispatcher or backend.

## [LSP-PULL-TRANSPORT] Idle Compiler Events

The shared framed reader separates responses from notifications without relying
on a request being active. Codec and envelope classification are supplied by the
consumer. The reader owns its background task and releases it when the session
ends. Transport and decode failures remain visible to the response consumer.

## [LSP-PULL-WATCH] Watcher Lifetime

Native file observations feed the existing shared coalescing scheduler. The
owned combined stream releases native watching and its forwarding task when
dropped; applications do not implement another debounce loop.

Consumers may subscribe to immediate children without recursively watching a
directory's descendants. Both depth modes preserve create/modify/remove events,
coalescing and owned release. A depth change requires replacing the subscription;
an existing shallow subscription cannot stand in for a recursive one.

Native queue overflow/rescan flags and observation errors emit a typed `Rescan`
event, including when the operating system supplies no paths. Consumers must
rescan their complete owned scope; an incomplete path subset cannot stand in
for this signal. Observation loss is logged with structured counts. Rescans
share normal coalescing and owned release with other native events.
