# Flat target contracts

This local suite exercises real Flat page allocation, reservation contention,
owner cancellation, transfer, stale version rejection and allocation-failure
recovery. Its `provider.patch` attaches a Kernel Task probe in a disposable
build without changing the public Kernel ABI.

```sh
verify run --suite server/tests/target/basis/flat \
  --kernel "$PWD" --output /tmp/flat --harts 1
```

QEMU runs single-Hart by default; a four-Hart run is explicit. The suite owns
the Guest, patch contract and result markers. It does not share test verdicts
with Nest, IOMMU or other projects.
