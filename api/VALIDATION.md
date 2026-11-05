# API refactor validation

Base: Kernel main `8a2c5fb9263fb1b42b2088dbb88ddcb37e1e7348`.
The root Server implementation, dispatch, authorization and scheduler source
are unchanged; its dependency graph now selects the canonical ABI owner.

Executed with nightly-2026-10-09:

| Check | Result |
| --- | --- |
| API workspace host tests | 68 passed; 4 inherited doctests ignored |
| Client memory and heap host fixture | 13 passed |
| Client task ownership/rollback fixture | 1 passed |
| Client TLS initialization, failure and retry fixture | 1 passed |
| Client renamed API and typed call contract fixture | 1 passed |
| API effect absence and TLS ownership compile-fail probes | 5 expected rejections |
| Client host and RV64 library compilation | passed |
| Actual root Server library and `shuyra` RV64 debug linking | passed |
| Actual root Server library and `shuyra` RV64 release linking, locked/offline | passed |

The contract fixture exercises narrow, fallible and triple-register returns.
It also sends `usize::MAX` through the generated infallible tick and clock
functions. An independent regression reproduced the initial high-bit failure
and passed after correction. Only variants with an error domain interpret
negative register values as errors; all typed entry points use API's single
`decode_return` implementation.

The resolved Server graph contains one owning `kernel-api-env`; its legacy
`env` dependency through `iommu-hold` is a re-export-only adapter. No original
Git `env`/wire definitions or Client transport enter the Server graph.
The Client contract fixture verifies nominal handle identity with a renamed
canonical API dependency. This is stronger than checking equal bytes alone.

Negative probes separately require the expected compiler diagnostics: API has
no trap or enum `.call()`, and TLS tokens implement none of Copy, Send or Sync.
They are not passing runtime tests. TLS fault injection executes the actual
Client TLS source with controlled effects; its native-Reaped branch models
Server terminal responsibility and does not prove synchronous physical cleanup.

Call numbers, payload definitions, error codes and wire widths are preserved.
The Rust source interface intentionally changes: effectful calls move from API
to `kernel_client::call`, while API retains data and pure codecs. External
product repositories using old `env` call functions are not switched by this
PR and require a separately validated dependency closure migration.

These checks do not establish QEMU boot, SMP execution, device operation or a
whole-product Runtime cutover. Existing root compiler warnings and the four
ignored doctests are retained; this is not a claim of a warning-free full suite.
