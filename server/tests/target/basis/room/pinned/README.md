# Published ABI13 Room workload

This Kernel-owned Guest preserves the original Room scheduling workload
and its complete teardown/worker/evidence assertions at the published ABI13
Kernel provider `789ffd19b5359f3de172fb9aad8a61f6a7113a64`. It does not claim current Kernel main/ABI14.

The Guest and Stack originate from that Kernel provider. The minimal test
support and QEMU virt/EDU platform startup were retained from Vanllia
`f2dfa14f5b1f7bd47ee84841ba5c7112e8475950`; the reviewed transport and Room
test-platform transformations have been incorporated. Neither the product
test Guest nor its test-specific startup tree is fetched from Vanllia at run
time. The production `platform-riscv` source remains an immutable Git build
dependency, not a test ownership dependency.

The QEMU CPU/RAM/virtio-blk profile remains the same. The source patch for
Kernel latency-window measurement is independently SHA-256 guarded and
does not grant Activity, IOMMU or other unqualified features. A passing Host
compilation does not establish target QEMU execution or benchmark improvement.
