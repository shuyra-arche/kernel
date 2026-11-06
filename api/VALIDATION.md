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

The resolved Server graph contains one owning `kernel-api-env`. Its current
`iommu-hold` pin is `90f59ef91d3c4c157fb78bb691991ee7abe4f175` from the canonical
`driver-iommu` repository, with direct canonical API dependency. Root patches
only that API package to `api/env`; no legacy `env` patch remains. No legacy
`env`/wire definitions or Client transport enter this Server graph.
The Client contract fixture verifies nominal handle identity with a renamed
canonical API dependency. This is stronger than checking equal bytes alone.

The consumer-alignment follow-up starts from published Kernel `a9a1d8d` and
resolves `iommu-hold@90f59ef` from its public canonical Git URL. The actual Kernel
Server RV64 library check and locked/offline release link are repeated with that
lock. Its normal/build tree contains exactly one local canonical API owner and
none of Client, execution, legacy `env`, `wire`, or `mold`. This evidence covers
Kernel's hold dependency; the Driver IOMMU Server's standalone old-SDK type
conflicts require its separate product overlay validation.

TLS contract documentation now permits legacy terminal handoff without changing
initialization or exit behavior. Kernel terminal ownership of Caller mapping
cleanup does not promise eager local-table reclamation: a retained Live slot may
remain until the address space ends when no owed or pending work triggers sweep.

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
