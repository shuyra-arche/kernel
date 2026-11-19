# CPU completion probes

`probe.rs` and `instructions.rs` are injected by the current Nest target's
reviewed source patch. They run on real Kernel Harts in QEMU and exercise
completion acknowledgements, bounded retries, retained Page obligations,
duplicate receipts and the distinct instruction-fence path.

The probes only belong to `target/basis/nest/completion/`. They are not
standalone tests or an alternate Kernel implementation.
