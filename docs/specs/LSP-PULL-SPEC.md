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

## [LSP-PULL-CANCELLATION] Request Lifetime

The shared dispatcher owns cancellation registration. A request is registered
before it is scheduled so cancellation cannot race ahead of registration.
Completion, cancellation and dropped work release that registration. Shutdown
cancels all outstanding requests. A workspace scan checks cancellation and its
snapshot revision between documents and before emitting partial or final results.

## [LSP-PULL-TRANSPORT] Idle Compiler Events

The shared framed reader separates responses from notifications without relying
on a request being active. Codec and envelope classification are supplied by the
consumer. The reader owns its background task and releases it when the session
ends. Transport and decode failures remain visible to the response consumer.

## [LSP-PULL-WATCH] Watcher Lifetime

Native file observations feed the existing shared coalescing scheduler. The
owned combined stream releases native watching and its forwarding task when
dropped; applications do not implement another debounce loop.
