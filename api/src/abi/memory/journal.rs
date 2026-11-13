//! Fixed-capacity request/result protocol. The live Task must own exactly one
//! journal for its lifetime and serialize admission, resource commit and result
//! recording. This does not execute syscalls, authorize resources or own cleanup.

use super::{Domain, Failure, Outcome, RESULT_CAPACITY, Reply, Request, RequestId};
use crate::model::identity::TaskId;

#[derive(Clone, Copy)]
struct Record {
    request: Request,
    reply: Option<Reply>,
}
impl Record {
    fn terminal(self) -> bool {
        self.reply
            .is_some_and(|reply| !matches!(reply.outcome(), Outcome::Pending(_)))
    }
}

/// Only a fresh admission returns this handle. It is not clonable, has no public
/// constructor and must stay with that accepted operation, not the reply copy.
#[derive(Debug)]
pub struct Accepted {
    actor: TaskId,
    request: Request,
}

#[derive(Debug)]
pub enum Admission {
    Execute(Accepted),
    /// The same operation already owns execution; do not start another one.
    InFlight(Option<Reply>),
    Replay(Reply),
}

/// No heap allocation, eviction of pending work, wrapping or reset operation.
/// Numeric Task identity must come from the kernel, never from a descriptor.
pub struct Journal {
    actor: TaskId,
    high_water: u64,
    records: [Option<Record>; RESULT_CAPACITY],
}
impl Journal {
    pub fn new(actor: TaskId) -> Option<Self> {
        (actor.get() > 0 && actor.get() <= isize::MAX as usize).then_some(Self {
            actor,
            high_water: 0,
            records: [None; RESULT_CAPACITY],
        })
    }

    pub const fn high_water(&self) -> u64 {
        self.high_water
    }

    fn authorize(&self, actor: TaskId) -> Result<(), Failure> {
        if actor == self.actor {
            Ok(())
        } else {
            Err(Failure::Denied)
        }
    }

    /// Reserve storage before any resource effect. Canonical Request equality
    /// excludes output addresses; changing the opcode/domain is a mismatch too.
    pub fn begin(&mut self, actor: TaskId, request: Request) -> Result<Admission, Failure> {
        self.authorize(actor)?;
        let id = request.id().ok_or(Failure::Invalid)?;
        if let Some(record) = self
            .records
            .iter()
            .flatten()
            .find(|r| r.request.id() == Some(id))
        {
            if record.request != request {
                return Err(Failure::RequestMismatch);
            }
            return Ok(if record.terminal() {
                Admission::Replay(record.reply.expect("terminal result"))
            } else {
                Admission::InFlight(record.reply)
            });
        }
        if id.get() <= self.high_water {
            return Err(Failure::ResultExpired);
        }
        let slot = self
            .records
            .iter()
            .position(Option::is_none)
            .or_else(|| {
                self.records
                    .iter()
                    .enumerate()
                    .filter_map(|(i, record)| {
                        record
                            .filter(|r| r.terminal())
                            .map(|r| (i, r.request.id().unwrap().get()))
                    })
                    .min_by_key(|(_, id)| *id)
                    .map(|(i, _)| i)
            })
            .ok_or(Failure::Busy)?;
        self.records[slot] = Some(Record {
            request,
            reply: None,
        });
        self.high_water = id.get();
        Ok(Admission::Execute(Accepted { actor, request }))
    }

    /// Record while still inside the resource commit boundary, before copying
    /// output. No allocation occurs. A terminal result is immutable; late or
    /// mismatched callbacks cannot turn a commit into rejection or erase it.
    /// The resource owner must separately check exact object/publication epochs.
    pub fn record(&mut self, accepted: &Accepted, reply: Reply) -> Result<(), Failure> {
        self.authorize(accepted.actor)?;
        if !reply.matches(accepted.request) || matches!(reply.outcome(), Outcome::Expired) {
            return Err(Failure::Invalid);
        }
        let record = self
            .records
            .iter_mut()
            .flatten()
            .find(|r| r.request.id() == accepted.request.id())
            .ok_or(Failure::ResultExpired)?;
        if record.request != accepted.request {
            return Err(Failure::RequestMismatch);
        }
        if record.terminal() {
            return if record.reply == Some(reply) {
                Ok(())
            } else {
                Err(Failure::Stale)
            };
        }
        if record.reply.is_some() && matches!(reply.outcome(), Outcome::Rejected(_)) {
            return Err(Failure::Invalid);
        }
        record.reply = Some(reply);
        Ok(())
    }

    /// None means admitted but no public result yet. An old absent record is
    /// expired, including skipped sequence numbers; a future ID is NoRegion.
    /// Caller death may drop this storage, never resource/hardware obligations.
    pub fn inspect(
        &self,
        actor: TaskId,
        domain: Domain,
        id: RequestId,
    ) -> Result<Option<Reply>, Failure> {
        self.authorize(actor)?;
        if let Some(record) = self
            .records
            .iter()
            .flatten()
            .find(|r| r.request.id() == Some(id))
        {
            if record.request.action().opcode().domain() != domain {
                return Err(Failure::NoRegion);
            }
            return Ok(record.reply);
        }
        Err(if id.get() <= self.high_water {
            Failure::ResultExpired
        } else {
            Failure::NoRegion
        })
    }
}
