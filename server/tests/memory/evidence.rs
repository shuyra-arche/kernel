//! Contract tests only. Synthetic attestations exercise exact matching and
//! failure retention boundaries, never claim physical hardware completion.
use super::proof::*;
use core::num::NonZeroU64;
fn n(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}
fn request() -> Request {
    Request {
        region: Region::new(n(1), 0x8000, 4096, MemoryKind::Ram).unwrap(),
        member: Member {
            publication: n(2),
            generation: n(3),
            endpoint: Endpoint::Cpu {
                nest: n(4),
                leases: n(5),
            },
            access: Access::new(6).unwrap(),
        },
        revision: n(6),
        attempt: n(7),
        scope: Scope::CpuAddressSpace,
    }
}
// SAFETY: synthetic evidence is confined to pure tests and never reaches hardware
// or live resources; assertions test matching, not the factual attestation.
fn evidence<K>(request: Request) -> Evidence<K> {
    unsafe { Evidence::attest(request) }
}

#[test]
fn defaults_never_attest_an_unknown_platform() {
    let r = request();
    let mut backend = Unsupported;
    assert!(matches!(
        backend.compatible(Group::new(r, &[]).unwrap()),
        Err(Error::Unsupported)
    ));
    assert!(matches!(backend.data_visible(r), Err(Error::Unsupported)));
    assert!(matches!(
        backend.instructions_visible(r),
        Err(Error::Unsupported)
    ));
    assert!(matches!(
        backend.translations_retired(r),
        Err(Error::Unsupported)
    ));
    assert!(matches!(
        backend.accesses_drained(r),
        Err(Error::Unsupported)
    ));
    assert!(matches!(
        backend.auxiliary_quiescent(r),
        Err(Error::Unsupported)
    ));
}

#[test]
fn admission_covers_every_member_and_rejects_mmio_unknown_and_writable_code() {
    let r = request();
    for kind in [MemoryKind::Mmio, MemoryKind::Unknown] {
        let bad = Request {
            region: Region::new(n(1), 0x8000, 4096, kind).unwrap(),
            ..r
        };
        assert!(matches!(Group::new(bad, &[]), Err(Error::Unsupported)));
        assert!(matches!(
            visible(bad, evidence(bad), evidence(bad), None),
            Err(Error::Unsupported)
        ));
    }
    let execute = Member {
        publication: n(9),
        access: Access::new(8).unwrap(),
        ..r.member
    };
    assert!(matches!(Group::new(r, &[execute]), Err(Error::Unsupported)));
    assert!(Access::new(14).is_none());
    assert!(matches!(Group::new(r, &[r.member]), Err(Error::Invalid)));
    let member = Member {
        publication: n(9),
        ..r.member
    };
    assert!(matches!(
        Group::new(r, &[member, member]),
        Err(Error::Invalid)
    ));
    let peers: Vec<_> = (10..10 + MAX_MEMBERS as u64)
        .map(|id| Member {
            publication: n(id),
            ..r.member
        })
        .collect();
    assert!(Group::new(r, &peers[..MAX_MEMBERS - 1]).is_ok());
    assert!(matches!(Group::new(r, &peers), Err(Error::Capacity)));
    assert!(Region::new(n(1), u64::MAX - 4095, 4096, MemoryKind::Ram).is_none());
    assert!(Region::new(n(1), 1, 4096, MemoryKind::Ram).is_none());
}

#[test]
fn every_identity_scope_and_attempt_participates_in_evidence_matching() {
    let r = request();
    let others = [
        Request { attempt: n(8), ..r },
        Request {
            revision: n(8),
            ..r
        },
        Request {
            scope: Scope::CpuGlobal,
            ..r
        },
        Request {
            region: Region::new(n(8), 0x8000, 4096, MemoryKind::Ram).unwrap(),
            ..r
        },
        Request {
            region: Region::new(n(1), 0x9000, 4096, MemoryKind::Ram).unwrap(),
            ..r
        },
        Request {
            member: Member {
                publication: n(8),
                ..r.member
            },
            ..r
        },
        Request {
            member: Member {
                generation: n(8),
                ..r.member
            },
            ..r
        },
        Request {
            member: Member {
                endpoint: Endpoint::Cpu {
                    nest: n(4),
                    leases: n(8),
                },
                ..r.member
            },
            ..r
        },
        Request {
            member: Member {
                access: Access::new(2).unwrap(),
                ..r.member
            },
            ..r
        },
    ];
    for old in others {
        assert!(matches!(
            retired(r, evidence(old), evidence(r)),
            Err(Error::Stale)
        ));
        assert!(matches!(
            retired(r, evidence(r), evidence(old)),
            Err(Error::Stale)
        ));
        assert!(matches!(
            visible(r, evidence(old), evidence(r), None),
            Err(Error::Stale)
        ));
        assert!(!retired(r, evidence(r), evidence(r)).unwrap().confirms(old));
    }
    assert!(retired(r, evidence(r), evidence(r)).unwrap().confirms(r));
}

#[test]
fn executable_visibility_and_external_draining_are_separate_obligations() {
    let r = request();
    let executable = Request {
        member: Member {
            access: Access::new(10).unwrap(),
            ..r.member
        },
        ..r
    };
    assert!(matches!(
        visible(executable, evidence(executable), evidence(executable), None),
        Err(Error::Stale)
    ));
    assert!(
        visible(
            executable,
            evidence(executable),
            evidence(executable),
            Some(evidence(executable))
        )
        .unwrap()
        .confirms(executable)
    );
    let device = Request {
        member: Member {
            endpoint: Endpoint::External {
                controller: 0x1000,
                requester: 0,
                binding: n(4),
            },
            ..r.member
        },
        scope: Scope::Publication,
        ..r
    };
    assert!(device.valid());
    let old = Request {
        member: Member {
            endpoint: Endpoint::External {
                controller: 0x1000,
                requester: 0,
                binding: n(5),
            },
            ..device.member
        },
        ..device
    };
    assert!(matches!(
        retired(device, evidence(device), evidence(old)),
        Err(Error::Stale)
    ));
    assert!(
        !Request {
            scope: Scope::CpuGlobal,
            ..device
        }
        .valid()
    );
    assert!(
        !Request {
            scope: Scope::ExternalBinding,
            ..r
        }
        .valid()
    );
}

struct GroupBackend;
// SAFETY: test fixture produces no evidence; it only refuses unsupported groups.
unsafe impl Backend for GroupBackend {
    fn compatible(&mut self, group: Group<'_>) -> Result<Option<Evidence<Compatibility>>, Error> {
        assert_eq!(group.request().region.kind(), MemoryKind::Ram);
        let devices = group
            .existing()
            .iter()
            .chain(core::iter::once(&group.request().member))
            .filter(|m| matches!(m.endpoint, Endpoint::External { .. }))
            .count();
        if devices > 1 {
            Err(Error::Unsupported)
        } else {
            Ok(None)
        }
    }
}
#[test]
fn two_cpu_device_pairs_do_not_imply_a_supported_three_member_group() {
    let r = request();
    let mut backend = GroupBackend;
    let device = |id| Member {
        publication: n(id),
        endpoint: Endpoint::External {
            controller: 0x1000,
            requester: id as u32,
            binding: n(id),
        },
        ..r.member
    };
    assert!(matches!(
        backend.compatible(Group::new(r, &[device(10)]).unwrap()),
        Ok(None)
    ));
    assert!(matches!(
        backend.compatible(Group::new(r, &[device(11)]).unwrap()),
        Ok(None)
    ));
    assert!(matches!(
        backend.compatible(Group::new(r, &[device(10), device(11)]).unwrap()),
        Err(Error::Unsupported)
    ));
}

#[test]
fn auxiliary_storage_needs_its_own_exact_quiescence_and_covering_scope() {
    let r = request();
    let publication = Request {
        scope: Scope::Publication,
        ..r
    };
    assert!(matches!(
        auxiliary(publication, evidence(publication)),
        Err(Error::Invalid)
    ));
    let old = Request { attempt: n(8), ..r };
    assert!(matches!(auxiliary(r, evidence(old)), Err(Error::Stale)));
    assert!(auxiliary(r, evidence(r)).unwrap().confirms(r));
    for kind in [MemoryKind::Mmio, MemoryKind::Unknown] {
        let invalid = Request {
            region: Region::new(n(1), 0x8000, 4096, kind).unwrap(),
            ..r
        };
        assert!(matches!(
            auxiliary(invalid, evidence(invalid)),
            Err(Error::Unsupported)
        ));
        assert!(matches!(
            retired(invalid, evidence(invalid), evidence(invalid)),
            Err(Error::Unsupported)
        ));
    }
}
