# Nest CPU completion target

This suite verifies real CPU mappings, retained Page obligations, withdrawal,
request replay, fault termination, address reuse and completion acknowledgements.
CPU data and instruction fence probes are imported from the relevant MMU
test helper directory. No assertion is inherited from another suite.

```sh
verify run --suite server/tests/target/basis/nest/completion \
  --kernel "$PWD" --output /tmp/nest --harts 1
```

Its `provider.patch` and exact source preimages belong to this directory.
Debug and Release single-Hart runs are part of ordinary CI. Four-Hart runs
are explicit and do not establish physical device or IOMMU safety.
