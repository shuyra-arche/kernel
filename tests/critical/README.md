# Execution-domain stop verification

The `critical-scene` embedded test deliberately remains halted. Its expected
runner outcome is a timeout; a timeout alone is not acceptance. `verify.py`
requires the guard diagnostic, peer-quiescence acknowledgement, their ordering,
and absence of the target reference-cleanup diagnostic.

From this repository root, validate the verifier itself with:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/critical
```

To inspect a native run, pass its qtest output and actual hart count:

```sh
python3 tests/critical/verify.py path/to/qtest.log 4
```

Native execution requires the external harness described in the root README,
`critical-scene`, and at least two running harts. Keep QEMU builds isolated.
Peer quiescence is acknowledged after vacating the address-space lease and
disabling interrupts, before non-returning HSM stop or fallback. The alarm's
coordinator election does not prove peers or devices stopped.

This test slays a held protected task. It does not prove deployed drivers are
protected or device DMA was drained; those need product-level acceptance.
