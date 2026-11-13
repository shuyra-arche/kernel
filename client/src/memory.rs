//! Flat/Nest calls for the next provider. v10 does not accept these calls.
//! Each invocation snapshots once and traps once. There is no automatic retry,
//! request ID allocation, permission cache or fallback to legacy Cell calls.
use kernel_api::abi::memory::{Call, Delivery, Failure, Frame, Observation, Reply, Request};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    Reply(Reply),
    Observation(Observation),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Output delivery failed; this says nothing about whether effects committed.
    Delivery(Failure),
    /// Unrecognized return registers, payload or request correlation. Effects
    /// remain unknown; query the original request instead of resubmitting it.
    Protocol,
}

pub mod flat {
    pub use kernel_api::abi::memory::flat::Call;
    pub fn invoke(call: Call) -> Result<super::Response, super::Error> {
        super::invoke(call.into())
    }
}
pub mod nest {
    pub use kernel_api::abi::memory::nest::Call;
    pub fn invoke(call: Call) -> Result<super::Response, super::Error> {
        super::invoke(call.into())
    }
}

/// Explicit transport for hosts and platform integrations. The callback gets
/// canonical bounded buffers, not permission to execute a different operation.
pub fn invoke_with(
    call: impl Into<Call>,
    transport: impl FnOnce(u64, &[u8; Request::WIDTH], &mut [u8; Reply::WIDTH]) -> [u64; 3],
) -> Result<Response, Error> {
    let call = call.into();
    let request = call.request();
    let input = request.to_bytes();
    let mut output = [0; Reply::WIDTH];
    let registers = transport(call.number(), &input, &mut output);
    match Delivery::from_registers(registers).ok_or(Error::Protocol)? {
        Delivery::Failed(error) => return Err(Error::Delivery(error)),
        Delivery::Written => {}
    }
    if let Some(reply) = Reply::from_bytes(&output).filter(|reply| reply.matches(request)) {
        return Ok(Response::Reply(reply));
    }
    if let Some(observation) =
        Observation::from_bytes(&output).filter(|value| value.matches(request))
    {
        return Ok(Response::Observation(observation));
    }
    Err(Error::Protocol)
}

fn invoke(call: Call) -> Result<Response, Error> {
    invoke_with(call, |_, input, output| {
        let frame = Frame::new(call, input.as_ptr() as usize, output.as_mut_ptr() as usize)
            .expect("bounded stack buffers");
        // SAFETY: input/output remain live for this one synchronous trap, with
        // exact descriptor sizes. As with other Client calls, use a Kernel Task.
        let (a, b, c) =
            unsafe { crate::transport::trap(frame.number() as usize, frame.registers()) };
        [a as u64, b as u64, c as u64]
    })
}
