# Nest CPU completion target test

The current Kernel's Nest guest verifies CPU access through real mappings,
withdrawal, preserved observer aliases, request-journal replay, actual Task
faults and safe address reuse. The separate CPU probes exercise completion
and instruction-fence acknowledgement under QEMU.

`verify nest` selects the supplied Kernel revision and applies only the
reviewed `nest-provider` source instrumentation inside a disposable archive.
Debug and Release one-Hart runs are part of ordinary CI. Multi-Hart runs
are explicit and are not implied by one-Hart passing.

Set `QEMU_RISCV64` and `QEMU_OPENSBI`, then run:

```sh
verify nest --kernel "$PWD" --output /tmp/nest --harts 1
verify nest --kernel "$PWD" --output /tmp/nest-multi --harts 4
```

These checks do not qualify physical devices or IOMMU retirement.
